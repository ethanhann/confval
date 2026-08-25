//! The environment provider.

use crate::diagnostic::Report;
use crate::format::Fields;
use crate::layering::nesting::{self, Leaf};
use crate::source::{SourceMap, Span};

/// Reads process environment variables into a neutral [`Fields`] tree.
///
/// Variables are selected by `prefix`, which is then stripped. A double
/// underscore separates nesting levels and a single underscore stays literal,
/// so `APP_SERVER__MAX_BODY_MB` under prefix `APP_` becomes
/// `server.max_body_mb`. Segments are lowercased. Each value is registered as
/// its own synthetic source, named for the variable such as `env:APP_PORT`, so
/// it carries a real span. The value is emitted as an unparsed literal for the
/// leaf parsers to coerce.
///
/// Prefix matching is case-sensitive and byte-exact, so write the prefix as
/// the variables begin, including its trailing underscore. An empty prefix
/// selects every variable in the process environment.
///
/// A prefix that matches nothing returns an empty level that contributes
/// nothing to a merge. The provider has no syntax-error failure mode, so it
/// returns `Some` and never `None`.
///
/// A variable that is not valid UTF-8 cannot be read as text. One whose name
/// carries the prefix is reported as an error, and one outside the prefix is
/// skipped.
#[hotpath::measure]
pub fn env_fields(sources: &mut SourceMap, prefix: &str, report: &mut Report) -> Option<Fields> {
    from_os_vars(sources, prefix, std::env::vars_os(), report)
}

/// Filters the raw environment down to UTF-8 pairs. `std::env::vars()` panics
/// while iterating when any variable in the process holds invalid Unicode,
/// even one unrelated to the prefix, so the provider reads `vars_os` and does
/// its own conversion.
fn from_os_vars(
    sources: &mut SourceMap,
    prefix: &str,
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    report: &mut Report,
) -> Option<Fields> {
    let mut utf8 = Vec::new();
    for (name, value) in vars {
        match (name.into_string(), value.into_string()) {
            (Ok(name), Ok(value)) => utf8.push((name, value)),
            (name, _) => {
                // Invalid bytes survive a lossy conversion as replacement
                // characters, so a name that carries the prefix still matches
                // it and gets the diagnostic. Anything else in the process
                // environment is not this provider's to judge.
                let lossy = match name {
                    Ok(name) => name,
                    Err(os_name) => os_name.to_string_lossy().into_owned(),
                };
                if lossy.starts_with(prefix) {
                    let source = sources.add(format!("env:{lossy}"), lossy.clone());
                    let span = Span::new(source, 0, lossy.len() as u32);
                    report
                        .error(format!("environment variable is not valid UTF-8: {lossy}"))
                        .at(span)
                        .emit();
                }
            }
        }
    }
    from_vars(sources, prefix, utf8, report)
}

fn from_vars(
    sources: &mut SourceMap,
    prefix: &str,
    vars: impl IntoIterator<Item = (String, String)>,
    report: &mut Report,
) -> Option<Fields> {
    let root = sources.add("env", String::new());
    let mut leaves = Vec::new();
    for (name, value) in vars {
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let path: Vec<String> = rest.split("__").map(|s| s.to_ascii_lowercase()).collect();
        if path.iter().any(|segment| segment.is_empty()) {
            let source = sources.add(format!("env:{name}"), name.clone());
            let span = Span::new(source, 0, name.len() as u32);
            report
                .error(format!("malformed environment variable name: {name}"))
                .at(span)
                .emit();
            continue;
        }
        let source = sources.add(format!("env:{name}"), value.clone());
        let span = Span::new(source, 0, value.len() as u32);
        leaves.push(Leaf {
            path,
            raw: value,
            source,
            span,
        });
    }
    Some(nesting::build(root, leaves, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FieldKind, Scalar, Value, ValueKind};

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn unparsed(field: &crate::format::Field) -> &str {
        let FieldKind::Value(Value {
            kind: ValueKind::Scalar(Scalar::Unparsed(raw)),
            ..
        }) = &field.kind
        else {
            panic!("expected an unparsed scalar");
        };
        raw
    }

    #[test]
    fn strips_the_prefix_and_lowercases_a_top_level_key() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = from_vars(
            &mut sources,
            "APP_",
            vars(&[("APP_PORT", "8080")]),
            &mut report,
        )
        .unwrap();

        // Assert
        assert_eq!(unparsed(fields.get("port").unwrap()), "8080");
        assert!(!report.has_issues());
    }

    #[test]
    fn double_underscore_nests_and_single_underscore_stays_literal() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = from_vars(
            &mut sources,
            "APP_",
            vars(&[("APP_SERVER__MAX_BODY_MB", "16")]),
            &mut report,
        )
        .unwrap();

        // Assert
        let FieldKind::Block(server) = &fields.get("server").unwrap().kind else {
            panic!("expected a nested block");
        };
        assert_eq!(unparsed(server.get("max_body_mb").unwrap()), "16");
    }

    #[test]
    fn a_non_matching_prefix_yields_an_empty_level() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = from_vars(
            &mut sources,
            "APP_",
            vars(&[("OTHER_PORT", "1")]),
            &mut report,
        )
        .unwrap();

        // Assert
        assert!(fields.get("port").is_none());
        assert_eq!(fields.iter().count(), 0);
    }

    #[test]
    fn a_trailing_separator_is_reported_as_malformed() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let _ = from_vars(
            &mut sources,
            "APP_",
            vars(&[("APP_SERVER__", "x")]),
            &mut report,
        );

        // Assert
        assert!(report.has_errors());
        let issue = &report.issues()[0];
        assert!(issue.message.contains("malformed"));
        let span = issue.span.expect("the error should carry a span");
        assert_eq!(sources.get(span.source).unwrap().name, "env:APP_SERVER__");
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_variable_outside_the_prefix_is_skipped() {
        use std::os::unix::ffi::OsStringExt;

        // Arrange
        // The process environment can hold arbitrary bytes. A variable outside
        // the prefix must not panic the provider or pollute the report.
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let os_vars = vec![
            (
                std::ffi::OsString::from("APP_PORT"),
                std::ffi::OsString::from("8080"),
            ),
            (
                std::ffi::OsString::from_vec(vec![b'O', b'T', b'H', b'E', b'R', 0xff]),
                std::ffi::OsString::from("x"),
            ),
        ];

        // Act
        let fields = from_os_vars(&mut sources, "APP_", os_vars, &mut report).unwrap();

        // Assert
        assert_eq!(unparsed(fields.get("port").unwrap()), "8080");
        assert_eq!(fields.iter().count(), 1);
        assert!(!report.has_issues());
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_variable_under_the_prefix_is_reported() {
        use std::os::unix::ffi::OsStringExt;

        // Arrange
        // A prefixed name with invalid bytes and a prefixed name whose value
        // has invalid bytes are both operator mistakes worth a diagnostic.
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let os_vars = vec![
            (
                std::ffi::OsString::from_vec(vec![b'A', b'P', b'P', b'_', b'P', 0xff]),
                std::ffi::OsString::from("1"),
            ),
            (
                std::ffi::OsString::from("APP_MODE"),
                std::ffi::OsString::from_vec(vec![0xfe, 0xfd]),
            ),
        ];

        // Act
        let fields = from_os_vars(&mut sources, "APP_", os_vars, &mut report).unwrap();

        // Assert
        assert_eq!(fields.iter().count(), 0);
        assert_eq!(report.issues().len(), 2);
        assert!(report.has_errors());
        for issue in report.issues() {
            assert!(
                issue.message.contains("not valid UTF-8"),
                "unexpected message: {}",
                issue.message
            );
            let span = issue.span.expect("the error should carry a span");
            let name = &sources.get(span.source).unwrap().name;
            assert!(name.starts_with("env:"), "unexpected source: {name}");
        }
    }

    #[test]
    fn registers_a_synthetic_source_so_the_value_span_renders() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = from_vars(
            &mut sources,
            "APP_",
            vars(&[("APP_PORT", "8080")]),
            &mut report,
        )
        .unwrap();

        // Assert
        let field = fields.get("port").unwrap();
        assert_eq!(sources.get(field.source).unwrap().name, "env:APP_PORT");
    }
}
