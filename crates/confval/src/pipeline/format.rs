//! The format constraint, `#[confval(format = ...)]`, the
//! [`Format`](crate::Format) trait, and the built-in formats that need no
//! dependency beyond `std`.
//!
//! A format type is a unit struct that implements [`Format`](crate::Format).
//! The derive and a handwritten spec both call
//! [`check_format`](crate::pipeline::check_format) or
//! [`check_each_format`](crate::pipeline::check_each_format) with the type as
//! a parameter, so a format carries no data and no instance is built. A
//! domain format such as a CIDR block or a URL is a consumer type that
//! implements the trait the same way.

use crate::diagnostic::Report;
use crate::schema::Constraint;
use crate::source::Located;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

/// A named check that a string parses as one kind of value.
///
/// `check` takes no `self`, so a format type cannot carry configuration. That
/// keeps the derive's call a plain turbofish and the schema's record a name
/// and a function. A parameterized format is a later extension.
pub trait Format {
    /// The name the message and the hover use, such as "IPv4 address".
    const NAME: &'static str;

    /// Whether the value parses as this format.
    fn check(value: &str) -> bool;
}

/// Reports `{field} is not a valid {NAME}: "{value}"` at the value's span
/// when the value does not parse as `T`. The value is quoted so an empty or
/// whitespace-only value is visible in the message.
pub fn check_format<T: Format>(value: &Located<String>, field: &str, report: &mut Report) {
    if T::check(&value.value) {
        return;
    }
    report
        .error(format!(
            "{field} is not a valid {}: \"{}\"",
            T::NAME,
            value.value
        ))
        .at(value.span)
        .help(format!("Set {field} to a valid {}", T::NAME))
        .emit();
}

/// Reports `invalid {NAME} in {field}: "{value}"` for each element that does
/// not parse as `T`, at that element's span.
///
/// The message names the list rather than one element, the way
/// [`KeywordSet::check_each_in`](crate::KeywordSet::check_each_in) does, so
/// it reads correctly whatever the list is called. This is the form
/// `#[confval(format = ...)]` generates for a list.
pub fn check_each_format<T: Format>(values: &[Located<String>], field: &str, report: &mut Report) {
    for value in values {
        if T::check(&value.value) {
            continue;
        }
        report
            .error(format!(
                "invalid {} in {field}: \"{}\"",
                T::NAME,
                value.value
            ))
            .at(value.span)
            .help(format!("Set each entry in {field} to a valid {}", T::NAME))
            .emit();
    }
}

/// The schema record for `T`, its name and its check taken from the one
/// type, so the two cannot disagree. The derive emits this for a recorded
/// field, and a handwritten `ToSchema` calls it the same way.
pub fn constraint<T: Format>() -> Constraint {
    Constraint::format(T::NAME, T::check)
}

/// An IPv4 address, such as `127.0.0.1`.
#[derive(Debug, Clone, Copy)]
pub struct Ipv4;

impl Format for Ipv4 {
    const NAME: &'static str = "IPv4 address";

    fn check(value: &str) -> bool {
        Ipv4Addr::from_str(value).is_ok()
    }
}

/// An IPv6 address, such as `::1`.
#[derive(Debug, Clone, Copy)]
pub struct Ipv6;

impl Format for Ipv6 {
    const NAME: &'static str = "IPv6 address";

    fn check(value: &str) -> bool {
        Ipv6Addr::from_str(value).is_ok()
    }
}

/// An IPv4 or IPv6 address.
#[derive(Debug, Clone, Copy)]
pub struct Ip;

impl Format for Ip {
    const NAME: &'static str = "IP address";

    fn check(value: &str) -> bool {
        IpAddr::from_str(value).is_ok()
    }
}

/// A path that starts with `/`.
#[derive(Debug, Clone, Copy)]
pub struct AbsolutePath;

impl Format for AbsolutePath {
    const NAME: &'static str = "absolute path";

    fn check(value: &str) -> bool {
        value.starts_with('/')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Check = fn(&str) -> bool;

    fn check<T: Format>(value: &str, field: &str) -> Report {
        let mut report = Report::new();
        check_format::<T>(&Located::detached(value.to_string()), field, &mut report);
        report
    }

    #[test]
    fn a_value_that_parses_passes() {
        // Arrange
        let cases: [(&str, Check); 5] = [
            ("127.0.0.1", Ipv4::check),
            ("::1", Ipv6::check),
            ("10.0.0.1", Ip::check),
            ("fe80::1", Ip::check),
            ("/var/log", AbsolutePath::check),
        ];

        // Act
        let results: Vec<bool> = cases.iter().map(|(value, check)| check(value)).collect();

        // Assert
        assert_eq!(results, vec![true; 5]);
    }

    #[test]
    fn a_value_that_does_not_parse_reports_the_format_name_and_the_value() {
        // Arrange
        let value = "300.1.1.1";

        // Act
        let report = check::<Ipv4>(value, "bind");

        // Assert
        assert_eq!(
            report.issues()[0].message,
            "bind is not a valid IPv4 address: \"300.1.1.1\""
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Set bind to a valid IPv4 address")
        );
    }

    #[test]
    fn each_built_in_rejects_its_own_bad_value() {
        // Arrange
        let cases: [(&str, Check); 5] = [
            ("::1", Ipv4::check),
            ("127.0.0.1", Ipv6::check),
            ("localhost", Ip::check),
            ("var/log", AbsolutePath::check),
            ("", AbsolutePath::check),
        ];

        // Act
        let results: Vec<bool> = cases.iter().map(|(value, check)| check(value)).collect();

        // Assert
        assert_eq!(results, vec![false; 5]);
    }

    #[test]
    fn check_each_format_names_the_list_and_reports_each_bad_element_at_its_own_span() {
        // Arrange
        let mut sources = crate::source::SourceMap::new();
        let id = sources.add("test.hcl", "allow = [\"10.0.0.1\", \"nope\", \"\"]");
        let bad = crate::source::Span::new(id, 21, 27);
        let empty = crate::source::Span::new(id, 29, 31);
        let values = vec![
            Located::detached("10.0.0.1".to_string()),
            Located::new("nope".to_string(), bad),
            Located::new(String::new(), empty),
        ];
        let mut report = Report::new();

        // Act
        check_each_format::<Ip>(&values, "allow", &mut report);

        // Assert
        assert_eq!(report.issues().len(), 2);
        assert_eq!(
            report.issues()[0].message,
            "invalid IP address in allow: \"nope\""
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Set each entry in allow to a valid IP address")
        );
        assert_eq!(report.issues()[0].span, Some(bad));
        assert_eq!(
            report.issues()[1].message,
            "invalid IP address in allow: \"\""
        );
        assert_eq!(report.issues()[1].span, Some(empty));
    }

    #[test]
    fn a_consumer_format_implements_the_trait() {
        // Arrange
        struct Even;
        impl Format for Even {
            const NAME: &'static str = "even number";
            fn check(value: &str) -> bool {
                value.parse::<u32>().is_ok_and(|n| n % 2 == 0)
            }
        }

        // Act
        let pass = check::<Even>("4", "n");
        let fail = check::<Even>("5", "n");

        // Assert
        assert!(!pass.has_issues());
        assert_eq!(
            fail.issues()[0].message,
            "n is not a valid even number: \"5\""
        );
    }

    #[test]
    fn the_schema_record_takes_its_name_and_check_from_the_type() {
        // Act
        let recorded = constraint::<Ipv4>();

        // Assert
        let Constraint::Format { name, check, .. } = recorded else {
            panic!("a format constraint");
        };
        assert_eq!(name, "IPv4 address");
        assert!(check.call("127.0.0.1"));
        assert!(!check.call("::1"));
    }
}
