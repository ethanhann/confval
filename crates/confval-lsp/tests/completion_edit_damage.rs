//! Applied-edit tests for value completion. Each case accepts an item the
//! way an editor would, applies the replace edit to the buffer, and checks
//! the result. The range-level suites pin where an edit lands. This suite
//! pins what the buffer holds afterward.
#![cfg(feature = "hcl")]

mod fixture;

use confval::schema::ToSchema;
use confval_lsp::handlers::{ClientSupport, Cx, completion};
use confval_lsp::{Frontend, LineIndex, PositionEncoding};
use lsp_types::CompletionTextEdit;

use fixture::ServerSpec;

const ENCODING: PositionEncoding = PositionEncoding::Utf8;

/// The completion items at a byte offset, through the real resolution.
fn items_at<F: Frontend>(
    frontend: &F,
    text: &str,
    offset: usize,
) -> Vec<lsp_types::CompletionItem> {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let schema = ServerSpec::schema();
    let index = LineIndex::new(text);
    completion(
        frontend,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    )
}

/// The buffer after accepting the named item, or the buffer unchanged when no
/// item with that label is offered.
fn accept<F: Frontend>(frontend: &F, text: &str, offset: usize, label: &str) -> String {
    let items = items_at(frontend, text, offset);
    let Some(item) = items.iter().find(|item| item.label == label) else {
        return text.to_string();
    };
    let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
        panic!("the item carries a replace edit");
    };
    let index = LineIndex::new(text);
    let start = index.offset_of(text, edit.range.start, ENCODING);
    let end = index.offset_of(text, edit.range.end, ENCODING);
    let mut result = text.to_string();
    result.replace_range(start..end, &edit.new_text);
    result
}

#[test]
fn a_cursor_at_the_end_of_a_quoted_yaml_element_offers_nothing() {
    // Arrange
    let frontend = confval_lsp::Yaml;
    let text = "modes:\n  - \"enforce\"\n";
    let offset =
        text.find('\n').unwrap() + text[text.find('\n').unwrap()..].find("\"\n").unwrap() + 1;

    // Act
    let items = items_at(&frontend, text, offset);

    // Assert
    assert!(
        items.is_empty(),
        "a zero-width cursor flush against the closing quote fuses, got: {:?}",
        items.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
}

#[test]
fn a_cursor_before_the_opening_quote_of_a_yaml_element_offers_nothing() {
    // Arrange
    let frontend = confval_lsp::Yaml;
    let text = "modes:\n  - \"enforce\"\n";
    let offset = text.find('"').unwrap();

    // Act
    let items = items_at(&frontend, text, offset);

    // Assert
    assert!(
        items.is_empty(),
        "a zero-width cursor flush against the opening quote fuses, got: {:?}",
        items.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
}

#[test]
fn a_cursor_on_the_yaml_sequence_dash_offers_nothing() {
    // Arrange
    let frontend = confval_lsp::Yaml;
    let text = "modes:\n  - enforce\n";
    let dash = text.find('-').unwrap();

    // Act, Assert
    for offset in [dash, dash + 1] {
        let items = items_at(&frontend, text, offset);
        assert!(
            items.is_empty(),
            "an item at offset {offset} would replace the sequence dash, got: {:?}",
            items.iter().map(|item| &item.label).collect::<Vec<_>>()
        );
    }
}

#[test]
fn accepting_inside_a_single_quoted_yaml_element_replaces_the_quotes() {
    // Arrange
    let frontend = confval_lsp::Yaml;
    let text = "modes:\n  - 'enf'\n";
    let offset = text.find("enf").unwrap() + 1;

    // Act
    let result = accept(&frontend, text, offset, "enforce");

    // Assert
    assert_eq!(result, "modes:\n  - \"enforce\"\n");
    assert!(
        frontend.parse_tree(&result).is_some(),
        "the accepted buffer parses, got: {result}"
    );
}

#[test]
fn accepting_after_the_yaml_dash_and_space_still_inserts() {
    // Arrange
    let frontend = confval_lsp::Yaml;
    let text = "modes:\n  - ";
    let offset = text.len();

    // Act
    let result = accept(&frontend, text, offset, "enforce");

    // Assert
    assert_eq!(result, "modes:\n  - \"enforce\"");
    assert!(
        frontend.parse_tree(&result).is_some(),
        "the accepted buffer parses, got: {result}"
    );
}

#[test]
fn a_zero_width_cursor_against_an_hcl_element_quote_offers_nothing() {
    // Arrange
    let frontend = confval_lsp::Hcl;
    let text = "hostname = \"api\"\nport = 1\nmodes = [\"enforce\", \"log\"]\n";
    let offset = text.find('[').unwrap();

    // Act
    let items = items_at(&frontend, text, offset);

    // Assert
    assert!(
        items.is_empty(),
        "an insertion flush against the opening quote fuses, got: {:?}",
        items.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
}

#[cfg(feature = "kdl")]
#[test]
fn accepting_on_a_bare_kdl_list_node_writes_the_separating_space() {
    // Arrange
    let frontend = confval_lsp::Kdl;
    let text = "modes";
    let offset = text.len();

    // Act
    let result = accept(&frontend, text, offset, "enforce");

    // Assert
    assert_eq!(result, "modes \"enforce\"");
    assert!(
        frontend.parse_tree(&result).is_some(),
        "the accepted buffer parses, got: {result}"
    );
}

#[cfg(feature = "kdl")]
#[test]
fn accepting_a_bool_on_a_bare_kdl_node_writes_the_separating_space() {
    // Arrange
    let frontend = confval_lsp::Kdl;
    let text = "tls";
    let offset = text.len();

    // Act
    let result = accept(&frontend, text, offset, "true");

    // Assert
    assert_eq!(result, "tls #true");
    assert!(
        frontend.parse_tree(&result).is_some(),
        "the accepted buffer parses, got: {result}"
    );
}

#[cfg(feature = "kdl")]
#[test]
fn accepting_inside_a_populated_kdl_list_replaces_one_element() {
    // Arrange
    let frontend = confval_lsp::Kdl;
    let text = "modes \"log\" \"enf\"";
    let offset = text.rfind("enf").unwrap() + 1;

    // Act
    let result = accept(&frontend, text, offset, "enforce");

    // Assert
    assert_eq!(result, "modes \"log\" \"enforce\"");
    assert!(
        frontend.parse_tree(&result).is_some(),
        "the accepted buffer parses, got: {result}"
    );
}
