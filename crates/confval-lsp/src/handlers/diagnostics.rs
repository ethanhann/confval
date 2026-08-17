//! The diagnostics handler.
//!
//! It parses the whole buffer, builds the typed spec, runs `validate_all`, checks
//! references against the document's labels, and maps each `Report` issue to an
//! LSP diagnostic. It runs the real pipeline rather than an approximation, so a
//! diagnostic the editor shows is a diagnostic the program would produce.

use lsp_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Uri};

use confval::diagnostic::{Issue, Report, Severity};
use confval::format::FromFields;
use confval::pipeline::{Validate, ValidateNested, check_references};
use confval::schema::{Schema, ToSchema};
use confval::source::SourceMap;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::Frontend;

/// Produces the diagnostics for a document.
///
/// `S` is the root spec, `frontend` its format, and `schema` the caller's
/// already-built schema, so a default expression evaluates once per server
/// rather than on every publish. The `uri` is the document's own URI, used for
/// the related locations of a cross-field issue.
pub fn diagnostics<S, F>(
    frontend: &F,
    schema: &Schema,
    text: &str,
    uri: &Uri,
    encoding: PositionEncoding,
) -> Vec<Diagnostic>
where
    S: FromFields + Validate + ValidateNested + ToSchema,
    F: Frontend,
{
    let mut sources = SourceMap::new();
    let id = sources.add("<document>", text);
    let mut report = Report::new();
    // The reference pass runs whenever a tree parses, even when `from_fields`
    // fails on an unrelated structural error, because a reference still checks
    // against the labels the text carries.
    if let Some(fields) = frontend.parse(&sources, id, &mut report) {
        if let Some(spec) = S::from_fields(&fields, &mut report) {
            spec.validate_all(&mut report);
        }
        check_references(&fields, schema, &mut report);
    }

    let index = LineIndex::new(text);
    report
        .issues()
        .iter()
        .map(|issue| to_diagnostic(issue, &index, text, uri, encoding))
        .collect()
}

/// Maps one `confval` issue to an LSP diagnostic.
fn to_diagnostic(
    issue: &Issue,
    index: &LineIndex,
    text: &str,
    uri: &Uri,
    encoding: PositionEncoding,
) -> Diagnostic {
    let range = match issue.span {
        Some(span) => index.range_of(text, span, encoding),
        // A spanless issue points at the whole first line rather than a
        // zero-width range at the origin.
        None => index.range_of_bytes(text, (0, text.find('\n').unwrap_or(text.len())), encoding),
    };
    let severity = match issue.severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
    };
    // The help becomes a related note at the diagnostic's own location, so the
    // message stays a single line. The secondary spans follow as their own
    // related notes.
    let mut related = Vec::new();
    if let Some(help) = &issue.help {
        related.push(DiagnosticRelatedInformation {
            location: Location {
                uri: uri.clone(),
                range,
            },
            message: help.clone(),
        });
    }
    for (span, label) in &issue.related {
        related.push(DiagnosticRelatedInformation {
            location: Location {
                uri: uri.clone(),
                range: index.range_of(text, *span, encoding),
            },
            message: label.clone(),
        });
    }
    Diagnostic {
        range,
        severity: Some(severity),
        message: issue.message.clone(),
        related_information: (!related.is_empty()).then_some(related),
        source: Some("confval".to_string()),
        ..Diagnostic::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use confval::prelude::*;

    use crate::frontends::Hcl;

    #[derive(confval::Spec)]
    struct Upstream {
        #[confval(label)]
        name: Located<String>,
        host: Located<String>,
    }

    #[derive(confval::Spec)]
    struct Rule {
        #[confval(references = upstream)]
        upstream: Located<String>,
    }

    #[derive(confval::Spec)]
    struct Gateway {
        #[confval(nested)]
        upstream: Vec<Located<Upstream>>,
        #[confval(nested)]
        rules: Vec<Located<Rule>>,
    }

    impl Validate for Upstream {
        fn validate(&self, _report: &mut Report) {}
    }

    impl Validate for Rule {
        fn validate(&self, _report: &mut Report) {}
    }

    impl Validate for Gateway {
        fn validate(&self, _report: &mut Report) {}
    }

    #[test]
    fn the_handler_reports_an_undefined_reference() {
        // Arrange
        let text = "upstream \"api\" {\n  host = \"h\"\n}\nrules {\n  upstream = \"nope\"\n}\n";
        let uri = Uri::from_str("file:///gateway.hcl").unwrap();

        // Act
        let produced = diagnostics::<Gateway, Hcl>(
            &Hcl,
            &Gateway::schema(),
            text,
            &uri,
            PositionEncoding::Utf16,
        );

        // Assert
        assert!(
            produced
                .iter()
                .any(|d| d.message == "no upstream named \"nope\""),
            "got: {:?}",
            produced.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
