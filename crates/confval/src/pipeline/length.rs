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
    /// Reports `{field} must be at least {min} characters` or `{field} must
    /// be at most {max} characters` at the value's span when the count falls
    /// outside the bound.
    pub fn check_located(&self, value: &Located<String>, field: &str, report: &mut Report) {
        let count = value.value.chars().count();
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
/// A bound with `max:` alone starts at zero, so it pairs with
/// `#[confval(non_empty)]` without the two constraints reporting the same
/// empty value. A `min:` above `max:` is a compile error.
///
/// ```rust
/// use confval::length_constraint;
///
/// length_constraint!(HOSTNAME_LEN, max: 253);
/// length_constraint!(PORT_NAME_LEN, min: 1, max: 15);
/// length_constraint!(LABEL_LEN, min: 1, max: 63, help: "Each DNS label is at most 63 characters.");
/// ```
#[macro_export]
macro_rules! length_constraint {
    ($name:ident, min: $min:expr, max: $max:expr, help: $help:literal) => {
        const $name: $crate::LengthConstraint = $crate::LengthConstraint {
            min: $min,
            max: $max,
            help: Some($help),
        };
        const _: () = assert!($min <= $max, "length_constraint! min is above max");
    };
    ($name:ident, min: $min:expr, max: $max:expr) => {
        length_constraint!($name, min: $min, max: $max, help: None);
    };
    ($name:ident, max: $max:expr, help: $help:literal) => {
        length_constraint!($name, min: 0, max: $max, help: $help);
    };
    ($name:ident, max: $max:expr) => {
        length_constraint!($name, min: 0, max: $max, help: None);
    };
    ($name:ident, min: $min:expr, max: $max:expr, help: None) => {
        const $name: $crate::LengthConstraint = $crate::LengthConstraint {
            min: $min,
            max: $max,
            help: None,
        };
        const _: () = assert!($min <= $max, "length_constraint! min is above max");
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    length_constraint!(HOSTNAME_LEN, min: 1, max: 253);
    length_constraint!(LABEL_LEN, min: 1, max: 63, help: "Each DNS label is at most 63 characters.");
    length_constraint!(CAPPED, max: 4);

    fn check(constraint: &LengthConstraint, value: &str, field: &str) -> Report {
        let mut report = Report::new();
        constraint.check_located(&Located::detached(value.to_string()), field, &mut report);
        report
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
