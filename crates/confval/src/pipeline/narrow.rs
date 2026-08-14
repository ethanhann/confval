//! Checked integer narrowing for lowering functions.
//!
//! Spec types store every integer as `i64` (the widest type the source
//! format produces). Runtime types use the exact width they need. These
//! helpers narrow between the two and are shaped to slot directly into
//! `#[confval(lower(from = ..., with = ...))]`.
//!
//! Lowering runs only after the error gate, so a value that does not fit
//! means a validation rule is missing, not that the operator made a typo.
//! Rather than truncating silently, the helpers report the failure at the
//! value's span and return `None`.
//!
//! The module also holds `keyword`, which lowers a validated keyword string
//! into the enum `keyword_enum!` generates, through the same `with` shape.

use crate::diagnostic::Report;
use crate::source::Located;
use std::time::Duration;

macro_rules! narrow_fns {
    ($plain:ident, $opt:ident, $target:ty) => {
        /// Narrow a located `i64` to the target width, reporting at the
        /// value's span if it does not fit.
        pub fn $plain(value: &Located<i64>, report: &mut Report) -> Option<$target> {
            match <$target>::try_from(value.value) {
                Ok(narrowed) => Some(narrowed),
                Err(_) => {
                    report
                        .error(format!(
                            "value {} is out of range for {}",
                            value.value,
                            stringify!($target)
                        ))
                        .at(value.span)
                        .emit();
                    None
                }
            }
        }

        /// Optional-field variant: `None` in, `Some(None)` out. The outer
        /// `Option` is the failure channel.
        pub fn $opt(value: &Option<Located<i64>>, report: &mut Report) -> Option<Option<$target>> {
            match value {
                Some(value) => $plain(value, report).map(Some),
                None => Some(None),
            }
        }
    };
}

narrow_fns!(i64_to_u16, opt_i64_to_u16, u16);
narrow_fns!(i64_to_u32, opt_i64_to_u32, u32);
narrow_fns!(i64_to_u64, opt_i64_to_u64, u64);
narrow_fns!(i64_to_usize, opt_i64_to_usize, usize);

/// Widen a located `i64` to `f64`. This is infallible (the `report` argument is
/// unused), but it is defined here so a `#[confval(lower(with = ...))]`
/// attribute, which cannot hold a bare `as` cast, has a function to name.
/// Values above 2^53 lose integer precision, which is harmless for the ratios
/// and rates this is used for.
pub fn i64_to_f64(value: &Located<i64>, _report: &mut Report) -> Option<f64> {
    Some(value.value as f64)
}

/// Convert a located `i64` count of seconds to a `Duration`, reporting at the
/// value's span if it is negative (out of range for `u64`). This routes the
/// conversion through the same checked narrow as the integer helpers, so a
/// negative duration is rejected rather than wrapping into a near-unbounded one.
pub fn i64_secs_to_duration(value: &Located<i64>, report: &mut Report) -> Option<Duration> {
    i64_to_u64(value, report).map(Duration::from_secs)
}

/// Optional-field variant of [`i64_secs_to_duration`]: `None` in, `Some(None)`
/// out. The outer `Option` is the failure channel.
pub fn opt_i64_secs_to_duration(
    value: &Option<Located<i64>>,
    report: &mut Report,
) -> Option<Option<Duration>> {
    match value {
        Some(value) => i64_secs_to_duration(value, report).map(Some),
        None => Some(None),
    }
}

/// Lower a validated keyword string into its enum, reading the `TryFrom<&str>`
/// that `keyword_enum!` generates.
///
/// Name it in a `with` attribute with a turbofish so the derive knows which
/// enum to parse into, for example:
///
/// `#[confval(lower(from = mode, with = narrow::keyword::<LimitMode>))]`.
///
/// The error branch is defensive. A keyword field is validated against
/// `T::keyword_set()`, the same set this `TryFrom` accepts, so a value reaching
/// the branch means the `keyword_set()` check was never wired into the
/// `Validate` impl, or the set and the enum disagree.
pub fn keyword<T>(value: &Located<String>, report: &mut Report) -> Option<T>
where
    T: for<'a> TryFrom<&'a str>,
{
    match T::try_from(value.value.as_str()) {
        Ok(parsed) => Some(parsed),
        Err(_) => {
            report
                .error(format!("unknown keyword: {}", value.value))
                .at(value.span)
                .help(
                    "the value was not checked against a keyword set, or the set and its enum disagree",
                )
                .emit();
            None
        }
    }
}

/// Lowers a list of validated keyword strings into a list of enum values,
/// through [`keyword`] per element.
///
/// Every element that fails is reported before the function returns, so an
/// operator sees all of them in one run. Any failure fails the whole list. That
/// matches every other helper here, which reports and returns `None`. A caller
/// that wants the elements that parsed calls [`keyword`] per element.
///
/// This is the bare `Vec<Located<String>>` shape. For the wrapped optional
/// list, see [`opt_keyword_list`].
pub fn keyword_list<T>(values: &[Located<String>], report: &mut Report) -> Option<Vec<T>>
where
    T: for<'a> TryFrom<&'a str>,
{
    let mut parsed = Vec::with_capacity(values.len());
    let mut ok = true;
    for value in values {
        match keyword(value, report) {
            Some(item) => parsed.push(item),
            None => ok = false,
        }
    }
    ok.then_some(parsed)
}

/// Optional-list variant of [`keyword_list`]: `None` in, `Some(None)` out. The
/// outer `Option` is the failure channel.
///
/// The other `opt_` helpers differ from their plain forms by an `Option`. This
/// one also unwraps a `Located`, because the wrapped list shape is
/// `Option<Located<Vec<Located<String>>>>`. The two shapes come from the two
/// list fields a spec can declare, a bare `Vec` and a wrapper that keeps the
/// list's own span.
pub fn opt_keyword_list<T>(
    value: &Option<Located<Vec<Located<String>>>>,
    report: &mut Report,
) -> Option<Option<Vec<T>>>
where
    T: for<'a> TryFrom<&'a str>,
{
    match value {
        Some(list) => keyword_list(&list.value, report).map(Some),
        None => Some(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lowering helpers need only the `TryFrom<&str>` that `keyword_enum!`
    /// generates, so the fixture supplies that alone.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Color {
        Red,
        Green,
    }

    impl TryFrom<&str> for Color {
        type Error = ();

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            match value {
                "red" => Ok(Color::Red),
                "green" => Ok(Color::Green),
                _ => Err(()),
            }
        }
    }

    fn located(values: &[&str]) -> Vec<Located<String>> {
        values
            .iter()
            .map(|value| Located::detached(value.to_string()))
            .collect()
    }

    #[test]
    fn keyword_list_lowers_every_element_in_order() {
        // Arrange
        let values = located(&["green", "red"]);
        let mut report = Report::new();

        // Act
        let parsed = keyword_list::<Color>(&values, &mut report);

        // Assert
        assert_eq!(parsed, Some(vec![Color::Green, Color::Red]));
        assert!(!report.has_errors());
    }

    #[test]
    fn keyword_list_reports_every_bad_element_before_failing() {
        // Arrange
        let values = located(&["red", "mauve", "puce"]);
        let mut report = Report::new();

        // Act
        let parsed = keyword_list::<Color>(&values, &mut report);

        // Assert
        assert_eq!(parsed, None);
        assert_eq!(report.issues().len(), 2);
        assert_eq!(report.issues()[0].message, "unknown keyword: mauve");
        assert_eq!(report.issues()[1].message, "unknown keyword: puce");
    }

    #[test]
    fn keyword_list_lowers_an_empty_list_to_an_empty_vec() {
        // Arrange
        let mut report = Report::new();

        // Act
        let parsed = keyword_list::<Color>(&[], &mut report);

        // Assert
        assert_eq!(parsed, Some(Vec::new()));
    }

    #[test]
    fn opt_keyword_list_maps_an_absent_field_to_some_none() {
        // Arrange
        let absent = None;
        let mut report = Report::new();

        // Act
        let parsed = opt_keyword_list::<Color>(&absent, &mut report);

        // Assert
        assert_eq!(parsed, Some(None));
        assert!(!report.has_errors());
    }

    #[test]
    fn opt_keyword_list_lowers_a_present_list() {
        // Arrange
        let present = Some(Located::detached(located(&["red"])));
        let mut report = Report::new();

        // Act
        let parsed = opt_keyword_list::<Color>(&present, &mut report);

        // Assert
        assert_eq!(parsed, Some(Some(vec![Color::Red])));
    }

    #[test]
    fn opt_keyword_list_fails_the_whole_field_on_a_bad_element() {
        // Arrange
        let present = Some(Located::detached(located(&["red", "mauve"])));
        let mut report = Report::new();

        // Act
        let parsed = opt_keyword_list::<Color>(&present, &mut report);

        // Assert
        assert_eq!(parsed, None);
        assert!(report.has_errors());
    }

    #[test]
    fn in_range_value_narrows() {
        let mut report = Report::new();
        let value = Located::detached(8080_i64);

        assert_eq!(i64_to_u16(&value, &mut report), Some(8080_u16));
        assert!(!report.has_errors());
    }

    #[test]
    fn out_of_range_value_reports_and_fails() {
        let mut report = Report::new();
        let value = Located::detached(70_000_i64);

        assert_eq!(i64_to_u16(&value, &mut report), None);
        assert!(report.has_errors());
        assert_eq!(
            report.issues()[0].message,
            "value 70000 is out of range for u16"
        );
    }

    #[test]
    fn negative_value_reports_and_fails() {
        let mut report = Report::new();
        let value = Located::detached(-1_i64);

        assert_eq!(i64_to_u64(&value, &mut report), None);
        assert!(report.has_errors());
    }

    #[test]
    fn out_of_range_error_carries_the_span() {
        let mut sources = crate::source::SourceMap::new();
        let id = sources.add("test.hcl", "port = 99999");
        let span = crate::source::Span {
            source: id,
            start: 7,
            end: 12,
        };
        let mut report = Report::new();
        let value = Located {
            value: 99999_i64,
            span,
        };

        assert_eq!(i64_to_u16(&value, &mut report), None);
        assert_eq!(report.issues()[0].span, Some(span));
    }

    #[test]
    fn u16_narrows_at_its_maximum() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(65_535_i64);

        // Act
        let narrowed = i64_to_u16(&value, &mut report);

        // Assert
        assert_eq!(narrowed, Some(65_535_u16));
        assert!(!report.has_errors());
    }

    #[test]
    fn u16_fails_one_past_its_maximum() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(65_536_i64);

        // Act
        let narrowed = i64_to_u16(&value, &mut report);

        // Assert
        assert_eq!(narrowed, None);
        assert_eq!(
            report.issues()[0].message,
            "value 65536 is out of range for u16"
        );
    }

    #[test]
    fn u16_narrows_zero() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(0_i64);

        // Act
        let narrowed = i64_to_u16(&value, &mut report);

        // Assert
        assert_eq!(narrowed, Some(0_u16));
        assert!(!report.has_errors());
    }

    #[test]
    fn u16_fails_at_negative_one() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(-1_i64);

        // Act
        let narrowed = i64_to_u16(&value, &mut report);

        // Assert
        assert_eq!(narrowed, None);
        assert!(report.has_errors());
    }

    #[test]
    fn i64_max_fits_u64() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(i64::MAX);

        // Act
        let narrowed = i64_to_u64(&value, &mut report);

        // Assert
        assert_eq!(narrowed, Some(i64::MAX as u64));
        assert!(!report.has_errors());
    }

    #[test]
    fn i64_min_fails_u16() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(i64::MIN);

        // Act
        let narrowed = i64_to_u16(&value, &mut report);

        // Assert
        assert_eq!(narrowed, None);
        assert!(report.has_errors());
    }

    #[test]
    fn a_value_past_u16_still_fits_u32() {
        // Arrange
        // 70_000 overflows u16 but fits u32, so this pins that the u32 helper
        // targets u32 rather than a narrower width.
        let mut report = Report::new();
        let value = Located::detached(70_000_i64);

        // Act
        let narrowed = i64_to_u32(&value, &mut report);

        // Assert
        assert_eq!(narrowed, Some(70_000_u32));
        assert!(!report.has_errors());
    }

    #[test]
    fn usize_narrows_a_non_negative_value() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(42_i64);

        // Act
        let narrowed = i64_to_usize(&value, &mut report);

        // Assert
        assert_eq!(narrowed, Some(42_usize));
        assert!(!report.has_errors());
    }

    #[test]
    fn usize_fails_on_a_negative_value() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached(-1_i64);

        // Act
        let narrowed = i64_to_usize(&value, &mut report);

        // Assert
        assert_eq!(narrowed, None);
        assert!(report.has_errors());
    }

    #[test]
    fn optional_absent_is_not_a_failure() {
        let mut report = Report::new();

        assert_eq!(opt_i64_to_usize(&None, &mut report), Some(None));
        assert!(!report.has_errors());
    }

    #[test]
    fn optional_present_narrows() {
        let mut report = Report::new();
        let value = Some(Located::detached(42_i64));

        assert_eq!(opt_i64_to_u32(&value, &mut report), Some(Some(42_u32)));
    }

    #[test]
    fn optional_out_of_range_fails() {
        let mut report = Report::new();
        let value = Some(Located::detached(-5_i64));

        assert_eq!(opt_i64_to_u32(&value, &mut report), None);
        assert!(report.has_errors());
    }

    #[test]
    fn seconds_convert_to_duration() {
        let mut report = Report::new();
        let value = Located::detached(60_i64);

        assert_eq!(
            i64_secs_to_duration(&value, &mut report),
            Some(Duration::from_secs(60))
        );
        assert!(!report.has_errors());
    }

    #[test]
    fn negative_seconds_report_rather_than_wrap() {
        let mut report = Report::new();
        let value = Located::detached(-1_i64);

        assert_eq!(i64_secs_to_duration(&value, &mut report), None);
        assert!(report.has_errors());
    }

    #[test]
    fn optional_absent_seconds_is_not_a_failure() {
        let mut report = Report::new();

        assert_eq!(opt_i64_secs_to_duration(&None, &mut report), Some(None));
        assert!(!report.has_errors());
    }

    #[test]
    fn optional_present_seconds_convert() {
        let mut report = Report::new();
        let value = Some(Located::detached(30_i64));

        assert_eq!(
            opt_i64_secs_to_duration(&value, &mut report),
            Some(Some(Duration::from_secs(30)))
        );
    }

    #[test]
    fn optional_negative_seconds_fail() {
        let mut report = Report::new();
        let value = Some(Located::detached(-5_i64));

        assert_eq!(opt_i64_secs_to_duration(&value, &mut report), None);
        assert!(report.has_errors());
    }

    #[test]
    fn i64_widens_to_f64() {
        let mut report = Report::new();
        let value = Located::detached(-42_i64);

        assert_eq!(i64_to_f64(&value, &mut report), Some(-42.0));
        assert!(!report.has_errors());
    }

    // A minimal `TryFrom<&str>` stands in for a `keyword_enum!` enum here.
    // `keyword` requires only the conversion, so this tests the helper without
    // coupling it to the macro. The macro-generated case is covered end to end
    // by the `common` example.
    #[derive(Debug, PartialEq)]
    enum Mode {
        Log,
    }

    impl TryFrom<&str> for Mode {
        type Error = ();

        fn try_from(value: &str) -> Result<Self, ()> {
            match value {
                "log" => Ok(Self::Log),
                _ => Err(()),
            }
        }
    }

    #[test]
    fn valid_keyword_lowers_to_its_variant() {
        // Arrange
        let mut report = Report::new();
        let value = Located::detached("log".to_string());

        // Act
        let lowered = keyword::<Mode>(&value, &mut report);

        // Assert
        assert_eq!(lowered, Some(Mode::Log));
        assert!(!report.has_errors());
    }

    #[test]
    fn unknown_keyword_returns_none_and_reports_at_the_span() {
        // Arrange
        let mut sources = crate::source::SourceMap::new();
        let id = sources.add("test.hcl", "mode = \"warn\"");
        let span = crate::source::Span {
            source: id,
            start: 8,
            end: 12,
        };
        let mut report = Report::new();
        let value = Located {
            value: "warn".to_string(),
            span,
        };

        // Act
        let lowered = keyword::<Mode>(&value, &mut report);

        // Assert
        assert_eq!(lowered, None);
        assert_eq!(report.issues()[0].span, Some(span));
    }
}
