//! The diagnostics handler.
//!
//! It builds the typed spec from the document's stored parse, runs
//! `validate_all`, checks references against the document's labels, and maps
//! each `Report` issue to an LSP diagnostic. It runs the real pipeline rather
//! than an approximation, so a diagnostic the editor shows is a diagnostic the
//! program would produce.

use lsp_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Uri};

use confval::diagnostic::{Issue, Report, Severity};
use confval::format::Fields;
use confval::pipeline::check_references;
use confval::schema::Schema;

use crate::binding::Validator;
use crate::encoding::{LineIndex, PositionEncoding};

/// Produces the diagnostics for a document.
///
/// `validator` is the root spec's erased validate pass, built once with
/// [`Validator::of`], and `schema` the caller's already-built schema, so a
/// default expression evaluates once per binding rather than on every publish.
/// `fields` and `parse_report` are the document's stored parse, produced by
/// [`Frontend::parse_buffer`](crate::frontend::Frontend::parse_buffer) when
/// the text changed, so a publish does not parse the buffer a second time.
/// The `uri` is the document's own URI, used for the related locations of a
/// cross-field issue. The line index is computed here from `text`.
pub fn diagnostics(
    validator: Validator,
    schema: &Schema,
    fields: Option<&Fields>,
    parse_report: &Report,
    uri: &Uri,
    text: &str,
    encoding: PositionEncoding,
) -> Vec<Diagnostic> {
    let index = &LineIndex::new(text);
    let mut report = Report::new();
    // The reference pass runs whenever a tree parsed, even when the validate
    // pass fails on an unrelated structural error, because a reference still
    // checks against the labels the text contains.
    if let Some(fields) = fields {
        validator.run(fields, &mut report);
        check_references(fields, schema, &mut report);
    }

    parse_report
        .issues()
        .iter()
        .chain(report.issues().iter())
        .map(|issue| to_diagnostic(issue, index, text, uri, encoding))
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

#[cfg(all(test, feature = "hcl"))]
mod tests {
    use super::*;
    use std::str::FromStr;

    use confval::prelude::*;

    use crate::frontend::Frontend;
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
        let (tree, report) = Hcl.parse_buffer(text);
        let produced = diagnostics(
            Validator::of::<Gateway>(),
            &Gateway::schema(),
            tree.as_ref(),
            &report,
            &uri,
            text,
            PositionEncoding::Utf16,
        );

        // Assert
        let diagnostic = produced
            .iter()
            .find(|d| d.message == "no upstream named \"nope\"")
            .unwrap_or_else(|| {
                panic!(
                    "got: {:?}",
                    produced.iter().map(|d| &d.message).collect::<Vec<_>>()
                )
            });
        assert_eq!(diagnostic.message, "no upstream named \"nope\"");
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostic.source.as_deref(), Some("confval"));
        assert!(
            diagnostic.related_information.is_some(),
            "the reference help becomes a related note"
        );
    }
}
