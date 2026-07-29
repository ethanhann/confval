//! The command line provider.

use crate::diagnostic::Report;
use crate::format::Fields;
use crate::layering::nesting::{self, Leaf};
use crate::source::{SourceMap, Span};

/// Reads command line flags into a neutral [`Fields`] tree.
///
/// Each flag uses the `--key=value` form. A dot separates nesting levels and a
/// segment keeps its underscores, so `--server.max_body_mb=16` becomes
/// `server.max_body_mb`. Each value is registered as its own synthetic source,
/// named for the flag such as `cli:server.max_body_mb`, so it carries a real
/// span. The value is emitted as an unparsed literal for the leaf parsers to
/// coerce. Arguments that are not `--key=value` flags are ignored, so you can
/// pass the whole argument list.
///
/// A flag-shaped token without a value, such as `--port` standing for the
/// mistyped `--port=8080`, is reported as a warning. A bare `--` stays
/// ignored, since it conventionally separates flags from positionals.
///
/// The provider has no syntax-error failure mode, so it returns `Some` and
/// never `None`.
pub fn cli_fields(
    sources: &mut SourceMap,
    args: impl IntoIterator<Item = String>,
    report: &mut Report,
) -> Option<Fields> {
    let root = sources.add("cli", String::new());
    let mut leaves = Vec::new();
    for arg in args {
        let Some(flag) = arg.strip_prefix("--") else {
            continue;
        };
        let Some((key, value)) = flag.split_once('=') else {
            // A bare `--` conventionally separates flags from positionals and
            // stays ignored. Any other flag-shaped token without a value is
            // more likely the typo `--port 8080` than a positional argument,
            // so it warns rather than configuring nothing silently.
            if !flag.is_empty() {
                let source = sources.add(format!("cli:{arg}"), arg.clone());
                let span = Span::new(source, 0, arg.len() as u32);
                report
                    .warning(format!("flag has no value: {arg}"))
                    .at(span)
                    .help(format!("use {arg}=value"))
                    .emit();
            }
            continue;
        };
        let path: Vec<String> = key.split('.').map(|s| s.to_string()).collect();
        if path.iter().any(|segment| segment.is_empty()) {
            let source = sources.add(format!("cli:{arg}"), arg.clone());
            let span = Span::new(source, 0, arg.len() as u32);
            report
                .error(format!("malformed flag: {arg}"))
                .at(span)
                .emit();
            continue;
        }
        let value = value.to_string();
        let source = sources.add(format!("cli:{key}"), value.clone());
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
    use crate::format::{Field, FieldKind, Scalar, Value, ValueKind};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn unparsed(field: &Field) -> &str {
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
    fn a_dotted_flag_sets_a_nested_field() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = cli_fields(&mut sources, args(&["--limits.mode=log"]), &mut report).unwrap();

        // Assert
        let FieldKind::Block(limits) = &fields.get("limits").unwrap().kind else {
            panic!("expected a nested block");
        };
        assert_eq!(unparsed(limits.get("mode").unwrap()), "log");
    }

    #[test]
    fn a_top_level_flag_sets_a_top_level_field() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = cli_fields(&mut sources, args(&["--port=8080"]), &mut report).unwrap();

        // Assert
        assert_eq!(unparsed(fields.get("port").unwrap()), "8080");
        assert!(!report.has_issues());
    }

    #[test]
    fn a_flag_without_a_value_is_warned_at_the_token() {
        // Arrange
        // `--port 8080` is the common typo for `--port=8080`, so the dropped
        // token warns rather than configuring nothing silently.
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = cli_fields(&mut sources, args(&["--port", "8080"]), &mut report).unwrap();

        // Assert
        assert_eq!(fields.iter().count(), 0);
        assert!(report.has_issues());
        assert!(!report.has_errors());
        let issue = &report.issues()[0];
        assert_eq!(issue.message, "flag has no value: --port");
        let span = issue.span.expect("the warning should carry a span");
        assert_eq!(sources.get(span.source).unwrap().name, "cli:--port");
    }

    #[test]
    fn a_bare_double_dash_stays_ignored() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = cli_fields(&mut sources, args(&["--", "--port=1"]), &mut report).unwrap();

        // Assert
        assert_eq!(unparsed(fields.get("port").unwrap()), "1");
        assert!(!report.has_issues());
    }

    #[test]
    fn a_malformed_flag_error_carries_a_span() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let _ = cli_fields(&mut sources, args(&["--limits..mode=log"]), &mut report);

        // Assert
        assert!(report.has_errors());
        let issue = &report.issues()[0];
        assert!(issue.message.contains("malformed"));
        let span = issue.span.expect("the error should carry a span");
        assert_eq!(
            sources.get(span.source).unwrap().name,
            "cli:--limits..mode=log"
        );
    }

    #[test]
    fn non_flag_arguments_are_ignored() {
        // Arrange
        let mut sources = SourceMap::new();
        let mut report = Report::new();

        // Act
        let fields = cli_fields(
            &mut sources,
            args(&["run", "--port=1", "positional"]),
            &mut report,
        )
        .unwrap();

        // Assert
        assert_eq!(fields.iter().count(), 1);
        assert_eq!(unparsed(fields.get("port").unwrap()), "1");
    }
}
