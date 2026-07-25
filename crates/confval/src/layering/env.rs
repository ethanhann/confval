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
/// A prefix that matches nothing returns an empty level that contributes
/// nothing to a merge. The provider has no syntax-error failure mode, so it
/// returns `Some` and never `None`.
pub fn env_fields(sources: &mut SourceMap, prefix: &str, report: &mut Report) -> Option<Fields> {
    from_vars(sources, prefix, std::env::vars(), report)
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
            report
                .error(format!("malformed environment variable name: {name}"))
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
        assert!(report.issues()[0].message.contains("malformed"));
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
