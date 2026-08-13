//! The diagnostics handler.
//!
//! It parses the whole buffer, builds the typed spec, runs `validate_all`, and
//! maps each `Report` issue to an LSP diagnostic. It runs the real pipeline
//! rather than an approximation, so a diagnostic the editor shows is a diagnostic
//! the program would produce.

use lsp_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Uri};

use confval::diagnostic::{Issue, Report, Severity};
use confval::format::FromFields;
use confval::pipeline::{Validate, ValidateNested};
use confval::source::SourceMap;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::Frontend;

/// Produces the diagnostics for a document.
///
/// `S` is the root spec, `frontend` its format. The `uri` is the document's own
/// URI, used for the related locations of a cross-field issue.
pub fn diagnostics<S, F>(
    frontend: &F,
    text: &str,
    uri: &Uri,
    encoding: PositionEncoding,
) -> Vec<Diagnostic>
where
    S: FromFields + Validate + ValidateNested,
    F: Frontend,
{
    let mut sources = SourceMap::new();
    let id = sources.add("<document>", text);
    let mut report = Report::new();
    if let Some(fields) = frontend.parse(&sources, id, &mut report)
        && let Some(spec) = S::from_fields(&fields, &mut report)
    {
        spec.validate_all(&mut report);
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
    // The help stays out of the message, which keeps the message a single clean
    // line, and becomes a related note at the diagnostic's own location. The
    // secondary spans follow as their own related notes.
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
