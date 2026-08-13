//! The pure handlers, tested directly against the fixture.

mod fixture;

use std::str::FromStr;

use lsp_types::{CompletionItemKind, CompletionTextEdit, HoverContents, Position, Range, Uri};

use confval::schema::ToSchema;
use confval_lsp::handlers::{completion, diagnostics, hover};
use confval_lsp::{Frontend, Hcl, Kdl, LineIndex, PositionEncoding, Toml};

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

/// The text a completion item inserts, read from its replace edit.
fn inserted(item: &lsp_types::CompletionItem) -> String {
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit.new_text.clone(),
        _ => item.insert_text.clone().unwrap_or_default(),
    }
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
    // The unknown-field diagnostic points at the exact span the pipeline
    // produced: `bogus` starts the third line.
    let bogus = found
        .iter()
        .find(|d| d.message.contains("bogus"))
        .expect("an unknown-field diagnostic");
    assert_eq!(
        bogus.range.start,
        Position {
            line: 2,
            character: 0
        }
    );
    // The keyword help is carried as related information, not appended to the
    // message, so the message stays a single clean line.
    let mode = found
        .iter()
        .find(|d| d.message.contains("mode"))
        .expect("a keyword diagnostic");
    assert!(
        !mode.message.contains("expected one of"),
        "help is not in the message: {}",
        mode.message
    );
    let related = mode
        .related_information
        .as_ref()
        .expect("the help as related information");
    assert!(
        related
            .iter()
            .any(|note| note.message.contains("expected one of: enforce, log, off")),
        "help appears in related information"
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
    assert!(
        value.contains("The TCP port the server listens on"),
        "the doc comment renders: {value}"
    );
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
    assert!(value.contains("Has a default."), "got: {value}");
    assert!(value.contains("Not set. Uses its default."), "got: {value}");
}

#[test]
fn a_repeated_block_stays_offered_while_a_single_block_is_dropped() {
    // Arrange
    let text = "limits {\n}\nrules {\n  prefix = \"/a\"\n}\n";
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
    assert!(
        labels.contains(&"rules".to_string()),
        "a repeated block stays offered: {labels:?}"
    );
    assert!(
        !labels.contains(&"limits".to_string()),
        "an already-set single block is dropped: {labels:?}"
    );
}

#[test]
fn a_map_body_offers_no_keys() {
    // Arrange
    let text = "headers = {\n  \n}\n";
    let offset = text.find("{\n").unwrap() + 3;
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
    assert!(
        items.is_empty(),
        "a map has open keys, so its body offers nothing"
    );
}

#[test]
fn toml_block_completion_inserts_a_table_header() {
    // Arrange
    let text = "";
    let tree = Toml.parse_tree(text);
    let context = Toml.resolve(tree.as_ref(), text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Toml,
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
        .expect("limits offered");
    assert_eq!(inserted(limits), "[limits]");
}

#[test]
fn kdl_scalar_completion_inserts_the_bare_name_form() {
    // Arrange
    let text = "";
    let tree = Kdl.parse_tree(text);
    let context = Kdl.resolve(tree.as_ref(), text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Kdl,
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let port = items
        .iter()
        .find(|item| item.label == "port")
        .expect("port offered");
    assert_eq!(inserted(port), "port ");
}

#[test]
fn toml_enum_value_completion_offers_the_allowed_strings() {
    // Arrange
    let text = "[limits]\nmode = \"e\"\n";
    let offset = text.find("\"e\"").unwrap() + 1;
    let tree = Toml.parse_tree(text);
    let context = Toml.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Toml,
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
}

#[test]
fn a_half_typed_name_completes_over_a_replace_range() {
    // Arrange
    // A half-typed name does not parse, so resolution scans the token and the
    // completion replaces it rather than inserting after it.
    let text = "wor";
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
    let workers = items
        .iter()
        .find(|item| item.label == "workers")
        .expect("workers offered");
    match &workers.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => {
            assert_eq!(
                edit.range,
                Range {
                    start: Position {
                        line: 0,
                        character: 0
                    },
                    end: Position {
                        line: 0,
                        character: 3
                    },
                }
            );
            assert_eq!(edit.new_text, "workers = ");
        }
        other => panic!("expected a replace edit, got: {other:?}"),
    }
}

#[test]
fn enum_completion_over_a_value_keeps_the_items_and_replaces_only_the_value() {
    // Arrange
    // The value sits one line above a sibling block. The enum members do not
    // prefix-match `loud`, so each carries a filter text equal to the value, and
    // the replace edit covers only the value, never reaching into `rules`.
    let text = "limits {\n  mode = \"loud\"\n}\nrules {\n  prefix = \"/a\"\n}\n";
    let offset = text.find("loud").unwrap() + 1;
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
    let log = items
        .iter()
        .find(|item| item.label == "log")
        .expect("log offered");
    assert_eq!(log.filter_text.as_deref(), Some("\"loud\""));
    let Some(CompletionTextEdit::Edit(edit)) = &log.text_edit else {
        panic!("expected a replace edit");
    };
    let start = index.offset_of(text, edit.range.start, ENCODING);
    let end = index.offset_of(text, edit.range.end, ENCODING);
    assert_eq!(
        &text[start..end],
        "\"loud\"",
        "the edit replaces only the value"
    );
    assert_eq!(edit.new_text, "\"log\"");
}

#[test]
fn body_completion_on_an_empty_line_inserts_at_the_cursor() {
    // Arrange
    // A blank line inside the block gives no token to replace, so the edit is a
    // zero-width insert at the cursor rather than a client-placed insertion.
    let text = "limits {\n  max_body_mb = 2048\n  \n}\n";
    let offset = text.find("\n  \n").unwrap() + 3;
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
    let mode = items
        .iter()
        .find(|item| item.label == "mode")
        .expect("mode offered");
    let Some(CompletionTextEdit::Edit(edit)) = &mode.text_edit else {
        panic!("expected a replace edit");
    };
    assert_eq!(edit.range.start, edit.range.end, "a zero-width insert");
    assert_eq!(edit.new_text, "mode = ");
}

/// The Markdown body of a hover.
fn markdown(hover: lsp_types::Hover) -> String {
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        other => panic!("expected markup hover, got: {other:?}"),
    }
}
