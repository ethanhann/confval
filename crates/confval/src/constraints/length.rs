//! The character length constraint, `#[confval(length = ...)]`, and the
//! `length_constraint!` macro.
//!
//! The count is `value.chars().count()`, the number of Unicode scalar values.
//! A consumer that needs a byte count, such as a DNS wire limit, writes its
//! own check in the `Validate` body.

use crate::diagnostic::Report;
use crate::source::Located;

/// An inclusive bound on the character count of a string, reporting at the
/// value's span with a generated or supplied help line.
#[derive(Debug, Clone, Copy)]
pub struct LengthConstraint {
    /// The smallest allowed character count.
    pub min: usize,
    /// The largest allowed character count.
    pub max: usize,
    /// A help line that replaces the generated suggestion.
    pub help: Option<&'static str>,
}

impl LengthConstraint {
    /// The character count `length` measures: the number of Unicode scalar
    /// values. The language server and the core check both count with this, so
    /// they agree on the unit.
    pub fn measure(text: &str) -> usize {
        text.chars().count()
    }

    /// Whether `text` is within the inclusive bound.
    pub fn admits(&self, text: &str) -> bool {
        let count = Self::measure(text);
        self.min <= count && count <= self.max
    }

    /// Reports `{field} must be at least {min} characters` or `{field} must
    /// be at most {max} characters` at the value's span when the count falls
    /// outside the bound.
    pub fn check_located(&self, value: &Located<String>, field: &str, report: &mut Report) {
        let count = Self::measure(&value.value);
        let (limit, kind) = if count < self.min {
            (self.min, "at least")
        } else if count > self.max {
            (self.max, "at most")
        } else {
            return;
        };
        let noun = if limit == 1 {
            "character"
        } else {
            "characters"
        };
        let help = self
            .help
            .map(String::from)
            .unwrap_or_else(|| format!("Set {field} to {kind} {limit} {noun}"));
        report
            .error(format!("{field} must be {kind} {limit} {noun}"))
            .at(value.span)
            .help(help)
            .emit();
    }
}

/// Define a named length constraint as a const.
///
/// A bound with `max:` alone starts at zero. Such a bound pairs with
/// `#[confval(non_empty)]`. The length bound rejects a value that is too short.
/// `non_empty` rejects a value that is empty or whitespace-only. A `min:` above
/// `max:` is a compile error.
///
/// The name takes attributes and a visibility, the way a `const` item does.
/// Write them inside the call, before the name. If you declare a constraint
/// with `pub` in one module, you can use it from another. A `#[doc]` on it
/// satisfies a crate that denies `missing_docs`.
///
/// ```rust
/// use confval::length_constraint;
///
/// length_constraint!(HOSTNAME_LEN, max: 253);
/// length_constraint!(PORT_NAME_LEN, min: 1, max: 15);
/// length_constraint!(LABEL_LEN, min: 1, max: 63, help: "Each DNS label is at most 63 characters.");
///
/// // The macro also answers to its full path, with no import.
/// confval::length_constraint!(CAPPED, max: 8);
/// ```
///
/// For example, a `bounds` module holds the constraints and the spec module
/// uses them:
///
/// ```rust
/// mod bounds {
///     confval::length_constraint!(
///         /// The hostname bound.
///         pub HOSTNAME_LEN, max: 253
///     );
///     confval::length_constraint!(
///         /// The label bound.
///         pub(crate) LABEL_LEN, min: 1, max: 63, help: "Each DNS label is at most 63 characters."
///     );
/// }
///
/// assert_eq!(bounds::HOSTNAME_LEN.max, 253);
/// assert_eq!(bounds::LABEL_LEN.min, 1);
/// ```
#[macro_export]
macro_rules! length_constraint {
    (@emit $(#[$meta:meta])* $vis:vis $name:ident, $min:expr, $max:expr, $help:expr) => {
        $(#[$meta])*
        $vis const $name: $crate::LengthConstraint = $crate::LengthConstraint {
            min: $min,
            max: $max,
            help: $help,
        };
        const _: () = assert!($min <= $max, "length_constraint! min is above max");
    };
    ($(#[$meta:meta])* $vis:vis $name:ident, min: $min:expr, max: $max:expr, help: $help:literal) => {
        $crate::length_constraint!(@emit $(#[$meta])* $vis $name, $min, $max,
            ::core::option::Option::Some($help));
    };
    ($(#[$meta:meta])* $vis:vis $name:ident, min: $min:expr, max: $max:expr) => {
        $crate::length_constraint!(@emit $(#[$meta])* $vis $name, $min, $max,
            ::core::option::Option::None);
    };
    ($(#[$meta:meta])* $vis:vis $name:ident, max: $max:expr, help: $help:literal) => {
        $crate::length_constraint!(@emit $(#[$meta])* $vis $name, 0, $max,
            ::core::option::Option::Some($help));
    };
    ($(#[$meta:meta])* $vis:vis $name:ident, max: $max:expr) => {
        $crate::length_constraint!(@emit $(#[$meta])* $vis $name, 0, $max,
            ::core::option::Option::None);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    length_constraint!(HOSTNAME_LEN, min: 1, max: 253);
    length_constraint!(LABEL_LEN, min: 1, max: 63, help: "Each DNS label is at most 63 characters.");
    length_constraint!(CAPPED, max: 4);

    mod visibility {
        length_constraint!(
            /// The hostname bound, declared here and used from the parent module.
            pub PUBLIC_HOSTNAME_LEN, max: 253
        );
        length_constraint!(
            /// The label bound, visible inside the crate.
            pub(crate) CRATE_LABEL_LEN, min: 1, max: 63, help: "Each DNS label is at most 63 characters."
        );
    }

    fn check(constraint: &LengthConstraint, value: &str, field: &str) -> Report {
        let mut report = Report::new();
        constraint.check_located(&Located::detached(value.to_string()), field, &mut report);
        report
    }

    #[test]
    fn a_pub_constant_is_reachable_from_the_parent_module() {
        // Arrange
        let constraint = &visibility::PUBLIC_HOSTNAME_LEN;
        let long = "a".repeat(254);

        // Act
        let report = check(constraint, &long, "hostname");

        // Assert
        assert_eq!(
            (constraint.min, constraint.max, constraint.help),
            (0, 253, None)
        );
        assert!(
            report.issues()[0]
                .message
                .contains("hostname must be at most 253 characters")
        );
    }

    #[test]
    fn a_pub_crate_constant_is_reachable_from_the_parent_module() {
        // Arrange
        let constraint = &visibility::CRATE_LABEL_LEN;

        // Act
        let report = check(constraint, "", "label");

        // Assert
        assert_eq!(
            (constraint.min, constraint.max, constraint.help),
            (1, 63, Some("Each DNS label is at most 63 characters."))
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Each DNS label is at most 63 characters.")
        );
    }

    #[test]
    fn a_count_inside_the_bound_passes() {
        // Arrange
        let short = "a";
        let long = "a".repeat(253);

        // Act
        let short_report = check(&HOSTNAME_LEN, short, "hostname");
        let long_report = check(&HOSTNAME_LEN, &long, "hostname");

        // Assert
        assert!(!short_report.has_issues());
        assert!(!long_report.has_issues());
    }

    #[test]
    fn a_count_below_min_reports_at_least() {
        // Arrange
        let value = "";

        // Act
        let report = check(&HOSTNAME_LEN, value, "hostname");

        // Assert
        assert_eq!(
            report.issues()[0].message,
            "hostname must be at least 1 character"
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Set hostname to at least 1 character")
        );
    }

    #[test]
    fn a_count_above_max_reports_at_most() {
        // Arrange
        let value = "a".repeat(254);

        // Act
        let report = check(&HOSTNAME_LEN, &value, "hostname");

        // Assert
        assert_eq!(
            report.issues()[0].message,
            "hostname must be at most 253 characters"
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Set hostname to at most 253 characters")
        );
    }

    #[test]
    fn a_max_only_bound_starts_at_zero() {
        // Arrange
        let empty = "";
        let long = "abcde";

        // Act
        let empty_report = check(&CAPPED, empty, "tag");
        let long_report = check(&CAPPED, long, "tag");

        // Assert
        assert!(!empty_report.has_issues());
        assert_eq!(
            long_report.issues()[0].message,
            "tag must be at most 4 characters"
        );
    }

    #[test]
    fn the_count_is_in_characters_not_bytes() {
        // Arrange
        let constraint = LengthConstraint {
            min: 1,
            max: 2,
            help: None,
        };
        let value = "éé";

        // Act
        let report = check(&constraint, value, "label");

        // Assert
        assert!(!report.has_issues(), "two characters in four bytes pass");
    }

    #[test]
    fn custom_help_replaces_the_generated_line() {
        // Arrange
        let value = "a".repeat(64);

        // Act
        let report = check(&LABEL_LEN, &value, "label");

        // Assert
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Each DNS label is at most 63 characters.")
        );
    }

    #[test]
    fn the_error_carries_the_value_span() {
        // Arrange
        let mut sources = crate::source::SourceMap::new();
        let id = sources.add("test.hcl", "hostname = \"\"");
        let span = crate::source::Span::new(id, 11, 13);
        let value = Located::new(String::new(), span);
        let mut report = Report::new();

        // Act
        HOSTNAME_LEN.check_located(&value, "hostname", &mut report);

        // Assert
        assert_eq!(report.issues()[0].span, Some(span));
    }
}
