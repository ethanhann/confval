//! The pure handlers, tested directly against the fixture.

mod fixture;

use std::str::FromStr;

use lsp_types::{CompletionItemKind, HoverContents, Uri};

use confval::schema::ToSchema;
use confval_lsp::handlers::{completion, diagnostics, hover};
use confval_lsp::{Frontend, Hcl, LineIndex, PositionEncoding};

use fixture::ServerSpec;

const ENCODING: PositionEncoding = PositionEncoding::Utf8;

/// Resolves a cursor and returns the pieces the completion and hover handlers
/// take.
fn at(text: &str, offset: usize) -> (Option<confval::format::Fields>, confval_lsp::CursorContext) {
    let tree = Hcl.parse_tree(text);
    let context = Hcl.resolve(tree.as_ref(), text, offset);
    (tree, context)
}

/// The labels of a set of completion items.
fn labels(items: &[lsp_types::CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.clone()).collect()
}

#[test]
fn diagnostics_report_the_pipeline_issues_at_their_ranges() {
    // Arrange
    let text = "hostname = \"api\"\nport = 99999\nbogus = 1\nlimits {\n  mode = \"nope\"\n}\n";
    let uri = Uri::from_str("file:///fixture.hcl").unwrap();

    // Act
    let found = diagnostics::<ServerSpec, Hcl>(&Hcl, text, &uri, ENCODING);

    // Assert
    let messages: Vec<&str> = found.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("port")),
        "expected a port range diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("unknown field: bogus")),
        "expected an unknown-field diagnostic, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("mode")),
        "expected a keyword diagnostic, got: {messages:?}"
    );
    // Every diagnostic points at a real span, not the zero-width default.
    assert!(
        found
            .iter()
            .all(|d| d.range.end.character > 0 || d.range.end.line > 0)
    );
}

#[test]
fn attribute_name_completion_offers_unset_root_fields() {
    // Arrange
    let text = "hostname = \"a\"\nport = 8080\n";
    let (tree, context) = at(text, text.len());
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Hcl,
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let labels = labels(&items);
    assert!(labels.contains(&"workers".to_string()));
    assert!(labels.contains(&"tls".to_string()));
    // The fields the operator already set are dropped.
    assert!(!labels.contains(&"hostname".to_string()));
    assert!(!labels.contains(&"port".to_string()));
}

#[test]
fn block_type_completion_offers_the_nested_block() {
    // Arrange
    let text = "";
    let (tree, context) = at(text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Hcl,
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let limits = items
        .iter()
        .find(|item| item.label == "limits")
        .expect("the limits block is offered");
    assert_eq!(limits.kind, Some(CompletionItemKind::STRUCT));
}

#[test]
fn enum_value_completion_offers_the_allowed_strings() {
    // Arrange
    let text = "limits {\n  mode = \"e\"\n}\n";
    let offset = text.find("\"e\"").unwrap() + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Hcl,
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let mut labels = labels(&items);
    labels.sort();
    assert_eq!(labels, vec!["enforce", "log", "off"]);
    assert!(
        items
            .iter()
            .all(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER))
    );
}

#[test]
fn hover_renders_a_set_field_with_its_type_and_constraint() {
    // Arrange
    let text = "port = 8080\n";
    let offset = text.find("port").unwrap() + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING);

    // Assert
    let value = markdown(hover.expect("a hover for port"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(value.contains("Between 1 and 65535"), "got: {value}");
    assert!(value.contains("Set by the configuration."), "got: {value}");
}

#[test]
fn hover_states_a_defaulted_field_is_not_set() {
    // Arrange
    // A half-typed name does not parse, so the field is absent from the tree.
    // `workers` carries a default, so hover reads it as defaulted, not set.
    let text = "workers";
    let (tree, context) = at(text, text.len());
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING);

    // Assert
    let value = markdown(hover.expect("a hover for workers"));
    assert!(value.contains("Not set; uses its default."), "got: {value}");
}

/// The Markdown body of a hover.
fn markdown(hover: lsp_types::Hover) -> String {
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        other => panic!("expected markup hover, got: {other:?}"),
    }
}
