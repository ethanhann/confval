//! The format constraint, `#[confval(format = ...)]`, the [`Format`] trait,
//! and the built-in formats that need no dependency beyond `std`.
//!
//! A format type is a unit struct that implements [`Format`]. The derive and
//! a handwritten spec both call [`check_format`] or [`check_each_format`]
//! with the type as a parameter, so a format carries no data and no instance
//! is built. A domain format such as a CIDR block or a URL is a consumer
//! type that implements the trait the same way.

use crate::diagnostic::Report;
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

/// Reports `{field} is not a valid {NAME}: {value}` at the value's span when
/// the value does not parse as `T`.
pub fn check_format<T: Format>(value: &Located<String>, field: &str, report: &mut Report) {
    if T::check(&value.value) {
        return;
    }
    report
        .error(format!(
            "{field} is not a valid {}: {}",
            T::NAME,
            value.value
        ))
        .at(value.span)
        .help(format!("Set {field} to a valid {}.", T::NAME))
        .emit();
}

/// Reports `{field} is not a valid {NAME}: {value}` for each element that
/// does not parse as `T`, at that element's span.
pub fn check_each_format<T: Format>(values: &[Located<String>], field: &str, report: &mut Report) {
    for value in values {
        check_format::<T>(value, field, report);
    }
}

/// Declares a format whose check is one `std` type's `FromStr`.
macro_rules! parsed_format {
    ($(#[$doc:meta])* $name:ident, $parsed:ty, $label:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy)]
        pub struct $name;

        impl Format for $name {
            const NAME: &'static str = $label;

            fn check(value: &str) -> bool {
                <$parsed>::from_str(value).is_ok()
            }
        }
    };
}

parsed_format!(
    /// An IPv4 address, such as `127.0.0.1`.
    Ipv4,
    Ipv4Addr,
    "IPv4 address"
);
parsed_format!(
    /// An IPv6 address, such as `::1`.
    Ipv6,
    Ipv6Addr,
    "IPv6 address"
);
parsed_format!(
    /// An IPv4 or IPv6 address.
    Ip,
    IpAddr,
    "IP address"
);

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

    fn check<T: Format>(value: &str, field: &str) -> Report {
        let mut report = Report::new();
        check_format::<T>(&Located::detached(value.to_string()), field, &mut report);
        report
    }

    #[test]
    fn a_value_that_parses_passes() {
        // Arrange
        let values = [
            check::<Ipv4>("127.0.0.1", "bind"),
            check::<Ipv6>("::1", "bind"),
            check::<Ip>("10.0.0.1", "bind"),
            check::<Ip>("fe80::1", "bind"),
            check::<AbsolutePath>("/var/log", "root"),
        ];

        // Act
        let failures: Vec<bool> = values.iter().map(Report::has_issues).collect();

        // Assert
        assert_eq!(failures, vec![false; 5]);
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
            "bind is not a valid IPv4 address: 300.1.1.1"
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Set bind to a valid IPv4 address.")
        );
    }

    #[test]
    fn each_built_in_rejects_its_own_bad_value() {
        // Arrange
        let cases = [
            check::<Ipv4>("::1", "a"),
            check::<Ipv6>("127.0.0.1", "b"),
            check::<Ip>("localhost", "c"),
            check::<AbsolutePath>("var/log", "d"),
            check::<AbsolutePath>("", "e"),
        ];

        // Act
        let failures: Vec<bool> = cases.iter().map(Report::has_errors).collect();

        // Assert
        assert_eq!(failures, vec![true; 5]);
    }

    #[test]
    fn check_each_format_reports_each_bad_element_at_its_own_span() {
        // Arrange
        let mut sources = crate::source::SourceMap::new();
        let id = sources.add("test.hcl", "allow = [\"10.0.0.1\", \"nope\", \"x\"]");
        let bad = crate::source::Span::new(id, 21, 27);
        let worse = crate::source::Span::new(id, 29, 32);
        let values = vec![
            Located::detached("10.0.0.1".to_string()),
            Located::new("nope".to_string(), bad),
            Located::new("x".to_string(), worse),
        ];
        let mut report = Report::new();

        // Act
        check_each_format::<Ip>(&values, "allow", &mut report);

        // Assert
        assert_eq!(report.issues().len(), 2);
        assert_eq!(
            report.issues()[0].message,
            "allow is not a valid IP address: nope"
        );
        assert_eq!(report.issues()[0].span, Some(bad));
        assert_eq!(report.issues()[1].span, Some(worse));
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
        assert_eq!(fail.issues()[0].message, "n is not a valid even number: 5");
    }
}
