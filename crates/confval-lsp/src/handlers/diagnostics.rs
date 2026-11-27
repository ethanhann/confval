//! The diagnostics handler.
//!
//! It builds the typed spec from the document's stored parse, runs
//! `validate_all`, checks references against the document's labels, and maps
//! each `Report` issue to an LSP diagnostic. It runs the real pipeline rather
//! than an approximation, so a diagnostic the editor shows is a diagnostic the
//! program would produce.

use std::any::Any;
use std::collections::HashMap;

use lsp_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Uri};

use confval::diagnostic::{Issue, Report, Severity};
use confval::format::Fields;
use confval::pipeline::check_references;
use confval::schema::Schema;
use confval::source::SourceId;

use crate::binding::Validator;
use crate::encoding::{LineIndex, PositionEncoding};
use crate::workspace::{DocumentIndex, IndexedDocument, WorkspaceReport};

/// Produces the diagnostics for a document.
///
/// `validator` is the root spec's erased validate pass, built once with
/// [`Validator::of`], and `schema` the caller's already-built schema, so a
/// default expression evaluates once per binding rather than on every publish.
/// `fields` and `parse_report` are the document's stored parse, produced when
/// the text changed, so a publish does not parse the buffer a second time.
/// The `uri` is the document's own URI, used for the related locations of a
/// cross-field issue. The line index is computed here from `text`.
///
/// The return value pairs the diagnostics with the spec the validate pass built,
/// boxed. The caller retains one lowered instance per document for the
/// workspace pass. The spec is `None` when the text did not parse or lowering
/// failed.
pub fn diagnostics(
    validator: Validator,
    schema: &Schema,
    fields: Option<&Fields>,
    parse_report: &Report,
    uri: &Uri,
    text: &str,
    encoding: PositionEncoding,
) -> (Vec<Diagnostic>, Option<Box<dyn Any>>) {
    let index = &LineIndex::new(text);
    let mut report = Report::new();
    let mut spec = None;
    // The reference pass runs whenever a tree parsed, even when the validate
    // pass fails on an unrelated structural error, because a reference still
    // checks against the labels the text contains.
    if let Some(fields) = fields {
        spec = validator.run(fields, &mut report);
        check_references(fields, schema, &mut report);
    }
    let diagnostics = parse_report
        .issues()
        .iter()
        .chain(report.issues().iter())
        .map(|issue| to_diagnostic(issue, index, text, uri, encoding))
        .collect();
    (diagnostics, spec)
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

/// Renders the workspace issues of one document.
///
/// `workspace` is the pass's report and `index` the snapshot it ran against,
/// so the two agree. The `uri` selects the issues the consumer attributed to
/// that document. Every span resolves its [`SourceId`] through the index to
/// its own document's text and URI, so a cross-file related span points at the
/// other document. An unresolvable primary span falls back to the first line
/// of the attributed document, the way the per-document mapping treats one.
/// An unresolvable related span is dropped.
pub fn workspace_diagnostics(
    workspace: &WorkspaceReport,
    index: &DocumentIndex,
    uri: &str,
    encoding: PositionEncoding,
) -> Vec<Diagnostic> {
    let Some(report) = workspace.get(uri) else {
        return Vec::new();
    };
    let Some(attributed) = index.get(uri) else {
        return Vec::new();
    };
    let sources = source_table(index);
    report
        .issues()
        .iter()
        .map(|issue| to_workspace_diagnostic(issue, &sources, attributed, encoding))
        .collect()
}

/// One open document the span table resolves to: its URI, its line index, and
/// its text.
struct Resolved<'a> {
    uri: &'a Uri,
    index: LineIndex,
    text: &'a str,
}

/// Builds the `SourceId` to document table from the index. A parsed document
/// contributes the source its fields carry. The server registers every
/// document under a distinct id, so the table resolves each one.
fn source_table(index: &DocumentIndex) -> HashMap<SourceId, Resolved<'_>> {
    let mut table = HashMap::new();
    for (_, document) in index.iter() {
        if let Some(fields) = document.fields() {
            let text = document.text();
            table.insert(
                fields.source(),
                Resolved {
                    uri: document.uri(),
                    index: LineIndex::new(text),
                    text,
                },
            );
        }
    }
    table
}

/// Resolves one span to its own document's location, or `None` when the span's
/// source is not an open document.
fn resolve_span(
    span: confval::source::Span,
    sources: &HashMap<SourceId, Resolved<'_>>,
    encoding: PositionEncoding,
) -> Option<Location> {
    let resolved = sources.get(&span.source)?;
    Some(Location {
        uri: resolved.uri.clone(),
        range: resolved.index.range_of(resolved.text, span, encoding),
    })
}

/// Maps one workspace issue to an LSP diagnostic, mirroring `to_diagnostic`'s
/// severity, help, and related handling, with every location resolved through
/// the span table.
fn to_workspace_diagnostic(
    issue: &Issue,
    sources: &HashMap<SourceId, Resolved<'_>>,
    attributed: &IndexedDocument,
    encoding: PositionEncoding,
) -> Diagnostic {
    let primary = resolve_span_opt(issue.span, sources, attributed, encoding);
    let mut related = Vec::new();
    if let Some(help) = &issue.help {
        related.push(DiagnosticRelatedInformation {
            location: primary.clone(),
            message: help.clone(),
        });
    }
    for (span, label) in &issue.related {
        if let Some(location) = resolve_span(*span, sources, encoding) {
            related.push(DiagnosticRelatedInformation {
                location,
                message: label.clone(),
            });
        }
    }
    Diagnostic {
        range: primary.range,
        severity: Some(match issue.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        message: issue.message.clone(),
        related_information: (!related.is_empty()).then_some(related),
        source: Some("confval".to_string()),
        ..Diagnostic::default()
    }
}

/// The issue's primary location: the span's own document, or, when the issue
/// is spanless or its source is closed, the attributed document's first line.
fn resolve_span_opt(
    span: Option<confval::source::Span>,
    sources: &HashMap<SourceId, Resolved<'_>>,
    attributed: &IndexedDocument,
    encoding: PositionEncoding,
) -> Location {
    if let Some(span) = span
        && let Some(location) = resolve_span(span, sources, encoding)
    {
        return location;
    }
    let text = attributed.text();
    Location {
        uri: attributed.uri().clone(),
        range: LineIndex::new(text).range_of_bytes(
            text,
            (0, text.find('\n').unwrap_or(text.len())),
            encoding,
        ),
    }
}

#[cfg(all(test, feature = "hcl"))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::str::FromStr;

    use confval::prelude::*;
    use confval::source::{SourceMap, Span};

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
        let (produced, spec) = diagnostics(
            Validator::of::<Gateway>(),
            &Gateway::schema(),
            tree.as_ref(),
            &report,
            &uri,
            text,
            PositionEncoding::Utf16,
        );

        // Assert
        assert!(
            spec.expect("the gateway lowers")
                .downcast_ref::<Gateway>()
                .is_some(),
            "the boxed spec is the concrete type"
        );
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

    /// An index with two parsed documents, the ids of their sources, and the
    /// id of a third source no entry serves.
    fn workspace_index() -> (DocumentIndex, SourceId, SourceId, SourceId) {
        let a_text = "service \"api\" {}\n";
        let b_text = "service \"api\" {\n  host = \"h\"\n}\n";
        let mut sources = SourceMap::new();
        let id_a = sources.add("file:///a.hcl", a_text);
        let id_b = sources.add("file:///b.hcl", b_text);
        let id_c = sources.add("file:///c.hcl", "");
        let mut index = DocumentIndex::new();
        for (uri_str, text, id) in [
            ("file:///a.hcl", a_text, id_a),
            ("file:///b.hcl", b_text, id_b),
        ] {
            let uri = Uri::from_str(uri_str).unwrap();
            let fields = Fields::new(id, Span::new(id, 0, 0), Vec::new());
            let document = IndexedDocument::new(
                uri.clone(),
                text.to_string(),
                Some(fields),
                Report::new(),
                Schema::new(None, Vec::new()),
                Some(0),
            );
            index.insert(uri.as_str(), document);
        }
        (index, id_a, id_b, id_c)
    }

    #[test]
    fn the_workspace_renderer_resolves_each_span_to_its_own_document() {
        // Arrange
        let (index, id_a, id_b, _) = workspace_index();
        let mut report = WorkspaceReport::default();
        report
            .report("file:///a.hcl")
            .error("duplicate service name")
            .at(Span::new(id_a, 0, 13))
            .related(Span::new(id_b, 0, 13), "first declared here")
            .emit();

        // Act
        let produced =
            workspace_diagnostics(&report, &index, "file:///a.hcl", PositionEncoding::Utf16);

        // Assert
        let diagnostic = produced.first().expect("the attributed issue renders");
        assert_eq!(diagnostic.range.start.line, 0);
        assert_eq!(diagnostic.message, "duplicate service name");
        let related = diagnostic.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1, "got: {related:?}");
        assert_eq!(related[0].location.uri.as_str(), "file:///b.hcl");
        assert_eq!(related[0].message, "first declared here");
    }

    #[test]
    fn a_spanless_workspace_issue_points_at_the_attributed_first_line() {
        // Arrange
        let (index, _, _, _) = workspace_index();
        let mut report = WorkspaceReport::default();
        report
            .report("file:///a.hcl")
            .error("acme needs tls_automation")
            .emit();

        // Act
        let produced =
            workspace_diagnostics(&report, &index, "file:///a.hcl", PositionEncoding::Utf16);

        // Assert
        let diagnostic = produced.first().expect("the attributed issue renders");
        assert_eq!(diagnostic.range.start.line, 0);
        assert_eq!(diagnostic.range.end.line, 0, "the range covers one line");
        assert!(diagnostic.related_information.is_none());
    }

    #[test]
    fn a_span_from_a_closed_source_falls_back_and_its_related_is_dropped() {
        // Arrange
        let (index, _, _, closed) = workspace_index();
        let mut report = WorkspaceReport::default();
        report
            .report("file:///a.hcl")
            .error("duplicate bind address")
            .at(Span::new(closed, 0, 5))
            .related(Span::new(closed, 0, 5), "first bound here")
            .emit();

        // Act
        let produced =
            workspace_diagnostics(&report, &index, "file:///a.hcl", PositionEncoding::Utf16);

        // Assert
        let diagnostic = produced.first().expect("the attributed issue renders");
        assert_eq!(
            diagnostic.range.start.line, 0,
            "an unresolvable primary span falls back to the attributed document"
        );
        assert!(
            diagnostic.related_information.is_none(),
            "got: {:?}",
            diagnostic.related_information
        );
    }
}
