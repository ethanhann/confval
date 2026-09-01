use crate::diagnostic::Report;
use crate::source::Located;
use std::fmt;

/// An inclusive numeric range a located value is checked against, reporting
/// at the value's span with a generated or supplied help line.
#[derive(Debug, Clone)]
pub struct RangeConstraint<T> {
    /// The smallest allowed value.
    pub min: T,
    /// The largest allowed value.
    pub max: T,
    /// A unit suffix for the generated message, such as "seconds".
    pub units: Option<&'static str>,
    /// A help line that replaces the generated suggestion.
    pub help: Option<&'static str>,
}

impl<T> RangeConstraint<T>
where
    T: PartialOrd + fmt::Display + Copy,
{
    /// A range with generated help and no units.
    pub const fn new(min: T, max: T) -> Self {
        Self {
            min,
            max,
            units: None,
            help: None,
        }
    }

    /// A range whose messages name the unit, such as "seconds".
    pub const fn with_units(min: T, max: T, units: &'static str) -> Self {
        Self {
            min,
            max,
            units: Some(units),
            help: None,
        }
    }

    /// Checks a located value, pushing an issue with a span to the report
    /// if it is out of range. The error names the field and the bound it
    /// violated. The help line restates that bound, using the configured
    /// `units` and `help` text when present.
    pub fn check_located(&self, value: &Located<T>, field: &'static str, report: &mut Report) {
        // NaN is unordered and compares `false` against both bounds, so the
        // range checks below would accept it. The self-comparison is the only
        // generic NaN test available here, and for integer `T` it is always
        // `false` and compiles away. Infinities need no special case, because
        // they fall outside any finite min or max.
        #[allow(clippy::eq_op)]
        let is_nan = value.value != value.value;
        if is_nan {
            report
                .error(format!("{field} must be a number"))
                .at(value.span)
                .emit();
            return;
        }
        let (limit, kind) = if value.value < self.min {
            (self.min, "at least")
        } else if value.value > self.max {
            (self.max, "at most")
        } else {
            return;
        };
        let help = self.help.map(String::from).unwrap_or_else(|| {
            let units = self
                .units
                .map(|units| format!(" {units}"))
                .unwrap_or_default();
            format!("Set {field} to {kind} {limit}{units}")
        });
        report
            .error(format!("{} must be {} {}", field, kind, limit))
            .at(value.span)
            .help(help)
            .emit();
    }
}

/// Define a named range constraint as a const. A `min:` above `max:` is a
/// compile error.
///
/// The macro takes attributes and a visibility before the name, the way a
/// `const` item does. Write them inside the call. A const declared `pub` or
/// `pub(crate)` is usable from any module that imports from the module
/// holding it. A `#[doc]` on it satisfies a crate that denies `missing_docs`.
///
/// ```rust
/// use confval::range_constraint;
///
/// range_constraint!(THREADS, usize, min: 1, max: 1024);
/// range_constraint!(PORT, u16, min: 1, max: 65535);
/// range_constraint!(INTERVAL, u64, min: 1, max: 3600, units: "seconds");
/// range_constraint!(WORKERS, usize, min: 1, max: 128, help: "Match this to your CPU core count.");
///
/// // The macro also answers to its full path, and needs no `RangeConstraint` import.
/// confval::range_constraint!(LIMITS, u32, min: 1, max: 10);
/// ```
///
/// For example, a `bounds` module holds the constraints. The parent module
/// names them:
///
/// ```rust
/// mod bounds {
///     confval::range_constraint!(
///         /// The listening port.
///         pub PORT, i64, min: 1, max: 65535
///     );
///     confval::range_constraint!(
///         /// The drain window.
///         pub(crate) DRAIN, i64, min: 0, max: 300, units: "seconds"
///     );
/// }
///
/// assert_eq!(bounds::PORT.max, 65535);
/// assert_eq!(bounds::DRAIN.units, Some("seconds"));
/// ```
#[macro_export]
macro_rules! range_constraint {
    (
        $(#[$meta:meta])* $vis:vis $name:ident, $T:ty,
        min: $min:expr, max: $max:expr
        $(, units: $units:literal)? $(, help: $help:literal)?
    ) => {
        $(#[$meta])*
        $vis const $name: $crate::RangeConstraint<$T> = $crate::RangeConstraint {
            min: $min,
            max: $max,
            units: $crate::range_constraint!(@opt $($units)?),
            help: $crate::range_constraint!(@opt $($help)?),
        };
        const _: () = {
            let min: $T = $min;
            let max: $T = $max;
            ::core::assert!(min <= max, "range_constraint! min is above max");
        };
    };
    (@opt $value:literal) => { ::core::option::Option::Some($value) };
    (@opt) => { ::core::option::Option::None };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Report;
    use crate::source::Located;

    range_constraint!(PORT, i64, min: 1, max: 65535);
    range_constraint!(THREADS, i64, min: 1, max: 1024);
    range_constraint!(INTERVAL, i64, min: 1, max: 3600, units: "seconds");
    range_constraint!(WORKERS, i64, min: 1, max: 128, help: "Match this to your CPU core count.");
    range_constraint!(TIMEOUT, i64, min: 1, max: 300, units: "seconds", help: "Keep this under 5 minutes for responsive shutdowns.");

    mod visibility {
        range_constraint!(
            /// The listening port, declared here and used from the parent module.
            pub PUBLIC_PORT, i64, min: 1, max: 65535
        );
        range_constraint!(
            /// The worker count, visible inside the crate.
            pub(crate) CRATE_WORKERS, i64, min: 1, max: 128, units: "workers"
        );
    }

    fn check(constraint: &RangeConstraint<i64>, value: i64, field: &'static str) -> Report {
        let mut report = Report::new();
        constraint.check_located(&Located::detached(value), field, &mut report);
        report
    }

    #[test]
    fn a_pub_constant_is_reachable_from_the_parent_module() {
        // Arrange
        let constraint = &visibility::PUBLIC_PORT;

        // Act
        let report = check(constraint, 0, "port");

        // Assert
        assert_eq!(
            (
                constraint.min,
                constraint.max,
                constraint.units,
                constraint.help
            ),
            (1, 65535, None, None)
        );
        assert!(
            report.issues()[0]
                .message
                .contains("port must be at least 1")
        );
    }

    #[test]
    fn a_pub_crate_constant_is_reachable_from_the_parent_module() {
        // Arrange
        let constraint = &visibility::CRATE_WORKERS;

        // Act
        let report = check(constraint, 64, "workers");

        // Assert
        assert_eq!(
            (
                constraint.min,
                constraint.max,
                constraint.units,
                constraint.help
            ),
            (1, 128, Some("workers"), None)
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn in_range_reports_nothing() {
        assert!(!check(&PORT, 80, "port").has_issues());
        assert!(!check(&PORT, 1, "port").has_issues());
        assert!(!check(&PORT, 65535, "port").has_issues());
    }

    #[test]
    fn below_min_reports_issue() {
        let report = check(&PORT, 0, "port");
        assert!(
            report.issues()[0]
                .message
                .contains("port must be at least 1")
        );
    }

    #[test]
    fn above_max_reports_issue() {
        let report = check(&THREADS, 2000, "threads");
        assert!(
            report.issues()[0]
                .message
                .contains("threads must be at most 1024")
        );
    }

    #[test]
    fn help_includes_units_when_present() {
        // Arrange
        let expected = "seconds";

        // Act
        let report = check(&INTERVAL, 0, "interval");

        // Assert
        let help = report.issues()[0].help.as_ref().expect("No help specified");
        assert!(help.contains(expected));
        assert_eq!(help, "Set interval to at least 1 seconds");
    }

    #[test]
    fn help_without_units_ends_without_a_trailing_space() {
        // Arrange
        let expected = "Set threads to at least 1";

        // Act
        let report = check(&THREADS, 0, "threads");

        // Assert
        let help = report.issues()[0].help.as_ref().expect("No help specified");
        assert_eq!(help, expected);
    }

    #[test]
    fn custom_help_overrides_generated_text() {
        let report = check(&WORKERS, 0, "workers");
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Match this to your CPU core count.")
        );
        let report = check(&TIMEOUT, 0, "timeout");
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Keep this under 5 minutes for responsive shutdowns.")
        );
    }

    fn check_f64(min: f64, max: f64, value: f64) -> Report {
        let mut report = Report::new();
        RangeConstraint::new(min, max).check_located(
            &Located::detached(value),
            "ratio",
            &mut report,
        );
        report
    }

    #[test]
    fn nan_is_rejected() {
        // NaN passes neither `< min` nor `> max`. It must still be rejected.
        let report = check_f64(0.0, 1.0, f64::NAN);
        assert!(report.has_errors());
        assert_eq!(report.issues()[0].message, "ratio must be a number");
    }

    #[test]
    fn infinities_are_out_of_range() {
        assert!(check_f64(0.0, 1.0, f64::INFINITY).has_errors());
        assert!(check_f64(0.0, 1.0, f64::NEG_INFINITY).has_errors());
    }

    #[test]
    fn finite_float_in_range_is_accepted() {
        assert!(!check_f64(0.0, 1.0, 0.5).has_issues());
    }
}
