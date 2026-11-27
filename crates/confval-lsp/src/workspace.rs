//! The workspace pass.
//!
//! A [`WorkspaceValidator`] validates the open documents of a workspace
//! together. It reads a [`DocumentIndex`] of the documents a binding serves
//! and reports into a [`WorkspaceReport`] keyed by document URI. The server
//! runs the pass after every open, change, and close. It merges the report
//! into the diagnostics it publishes per document.

use std::any::Any;
use std::collections::BTreeMap;

use lsp_types::Uri;

use confval::diagnostic::Report;
use confval::format::Fields;
use confval::schema::Schema;

/// Validates the open documents of a workspace together.
///
/// The server calls `validate` after every open, change, and close, with a
/// read view of the documents a binding serves and their lowered specs. The
/// validator reports only the checks the per-document pass cannot run. The
/// two passes therefore report no issue twice. Every issue is attributed to a
/// document URI when it is pushed. The report key is that document.
pub trait WorkspaceValidator: Send {
    /// Validates the workspace's open documents, appending every finding to
    /// the report under the document each one is attributed to.
    fn validate(&self, documents: &DocumentIndex, report: &mut WorkspaceReport);
}

/// One open document the index serves: its text, parse, schema, and lowered
/// spec.
///
/// The server builds an entry when a document opens and refreshes it on every
/// change. `fields` is `None` when the text does not parse. `spec` is `None`
/// when the text does not parse or lowering failed.
pub struct IndexedDocument {
    uri: Uri,
    text: String,
    fields: Option<Fields>,
    parse_report: Report,
    schema: Schema,
    binding: Option<usize>,
    spec: Option<Box<dyn Any>>,
}

impl IndexedDocument {
    pub(crate) fn new(
        uri: Uri,
        text: String,
        fields: Option<Fields>,
        parse_report: Report,
        schema: Schema,
        binding: Option<usize>,
    ) -> Self {
        Self {
            uri,
            text,
            fields,
            parse_report,
            schema,
            binding,
            spec: None,
        }
    }

    /// The document's URI.
    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// The document's current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The parsed field tree, or `None` when the text does not parse.
    pub fn fields(&self) -> Option<&Fields> {
        self.fields.as_ref()
    }

    /// The report the parse produced.
    pub fn parse_report(&self) -> &Report {
        &self.parse_report
    }

    /// The matched binding's schema.
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The lowered spec the validate pass built, downcastable to the
    /// consumer's spec types, or `None` when lowering failed.
    pub fn spec(&self) -> Option<&dyn Any> {
        self.spec.as_deref()
    }

    pub(crate) fn binding(&self) -> Option<usize> {
        self.binding
    }

    pub(crate) fn set_spec(&mut self, spec: Option<Box<dyn Any>>) {
        self.spec = spec;
    }
}

/// The open documents of a workspace, keyed by URI string.
///
/// The index holds only the documents a binding serves. An unmatched document
/// stays outside, because an issue targeted at it would publish nowhere.
/// Iteration is in URI order, so a validator's output order is stable across
/// runs.
#[derive(Default)]
pub struct DocumentIndex {
    documents: BTreeMap<String, IndexedDocument>,
}

impl DocumentIndex {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The document with this URI string, or `None`.
    pub fn get(&self, uri: &str) -> Option<&IndexedDocument> {
        self.documents.get(uri)
    }

    /// Every document, in URI order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &IndexedDocument)> + '_ {
        self.documents
            .iter()
            .map(|(uri, document)| (uri.as_str(), document))
    }

    /// Whether the index holds no documents.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// The number of documents the index holds.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub(crate) fn insert(&mut self, uri: &str, document: IndexedDocument) {
        self.documents.insert(uri.to_string(), document);
    }

    pub(crate) fn remove(&mut self, uri: &str) -> Option<IndexedDocument> {
        self.documents.remove(uri)
    }

    pub(crate) fn get_mut(&mut self, uri: &str) -> Option<&mut IndexedDocument> {
        self.documents.get_mut(uri)
    }
}

/// The issues a workspace pass produced, keyed by the document each issue is
/// attributed to.
///
/// A consumer builds issues through [`report`](Self::report) with the usual
/// `Report::error`, `Report::warning`, and `IssueBuilder`, including spans in
/// other documents. The key is the document the consumer attributes the issue
/// to.
#[derive(Debug, Default)]
pub struct WorkspaceReport {
    issues: BTreeMap<String, Report>,
}

impl WorkspaceReport {
    /// The report of the document this URI string names, created empty when
    /// the validator has not reported into it yet.
    pub fn report(&mut self, uri: &str) -> &mut Report {
        self.issues.entry(uri.to_string()).or_default()
    }

    /// The report of the document this URI string names, or `None` when the
    /// validator reported nothing into it.
    pub fn get(&self, uri: &str) -> Option<&Report> {
        self.issues.get(uri)
    }

    /// Every reported document, in URI order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Report)> + '_ {
        self.issues
            .iter()
            .map(|(uri, report)| (uri.as_str(), report))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::str::FromStr;

    #[test]
    fn the_report_creates_on_demand_and_iterates_in_uri_order() {
        // Arrange
        let mut report = WorkspaceReport::default();

        // Act
        report.report("file:///b.hcl").error("b").emit();
        report.report("file:///a.hcl").warning("a").emit();

        // Assert
        let uris: Vec<&str> = report.iter().map(|(uri, _)| uri).collect();
        assert_eq!(uris, ["file:///a.hcl", "file:///b.hcl"]);
        assert_eq!(report.get("file:///a.hcl").unwrap().issues().len(), 1);
        assert!(report.get("file:///c.hcl").is_none());
    }

    #[test]
    fn the_index_iterates_in_uri_order_and_answers_lookups() {
        // Arrange
        let mut index = DocumentIndex::new();
        for name in ["b.hcl", "a.hcl"] {
            let uri = Uri::from_str(&format!("file:///{name}")).unwrap();
            let document = IndexedDocument::new(
                uri.clone(),
                String::new(),
                None,
                Report::new(),
                Schema::new(None, Vec::new()),
                Some(0),
            );
            index.insert(uri.as_str(), document);
        }

        // Act, Assert
        assert_eq!(index.len(), 2);
        let uris: Vec<&str> = index.iter().map(|(uri, _)| uri).collect();
        assert_eq!(uris, ["file:///a.hcl", "file:///b.hcl"]);
        assert!(index.get("file:///a.hcl").is_some());
        assert!(index.get("file:///c.hcl").is_none());
        assert!(!index.is_empty());
    }

    #[test]
    fn the_entry_round_trips_its_accessors_and_spec() {
        // Arrange
        let uri = Uri::from_str("file:///a.hcl").unwrap();
        let mut document = IndexedDocument::new(
            uri,
            "text".to_string(),
            None,
            Report::new(),
            Schema::new(None, Vec::new()),
            Some(3),
        );

        // Act
        document.set_spec(Some(Box::new(7_u32)));

        // Assert
        assert_eq!(document.uri().as_str(), "file:///a.hcl");
        assert_eq!(document.text(), "text");
        assert!(document.fields().is_none());
        assert!(!document.parse_report().has_issues());
        assert_eq!(document.binding(), Some(3));
        let spec = document.spec().expect("the spec is set");
        assert_eq!(spec.downcast_ref::<u32>(), Some(&7));
    }
}
