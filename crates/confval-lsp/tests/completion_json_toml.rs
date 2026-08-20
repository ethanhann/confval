//! The completion handler for the JSON and TOML frontends: table headers and
//! arrays of tables, quoted keys that absorb the opening quote, non-destructive
//! member inserts, array element objects, and container-opening inserts.

mod fixture;
mod support;

use lsp_types::{CompletionTextEdit, InsertTextFormat, Position, Range};

use confval::schema::ToSchema;
use confval_lsp::handlers::{ClientSupport, Cx, completion};
use confval_lsp::{Frontend, Json, LineIndex, Toml};

use fixture::ServerSpec;
use support::{ENCODING, at_with, inserted, labels};

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
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let limits = items
        .iter()
        .find(|item| item.label == "limits")
        .expect("limits offered");
    assert_eq!(inserted(limits), "[limits]");
}

#[test]
fn completion_inside_a_toml_array_of_tables_offers_the_block_fields() {
    // Arrange
    // The cursor is on the blank line inside a `[[rules]]` element with no
    // prefix set, so completion offers the element's fields.
    let text = "[[rules]]\n\n";
    let offset = text.len() - 1;
    let tree = Toml.parse_tree(text);
    let context = Toml.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Toml,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    assert!(
        labels(&items).contains(&"prefix".to_string()),
        "offers the rule fields: {:?}",
        labels(&items)
    );
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
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let mut labels = labels(&items);
    labels.sort();
    assert_eq!(labels, vec!["enforce", "log", "off"]);
}

#[test]
fn completing_a_typed_toml_header_replaces_the_bracket() {
    // Arrange
    // `[lim` does not parse, so completing must replace the typed bracket rather
    // than double it into `[[limits]`.
    let text = "[lim";
    let tree = Toml.parse_tree(text);
    assert!(tree.is_none(), "the partial header does not parse");
    let context = Toml.resolve(tree.as_ref(), text, text.len());
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Toml,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let limits = items
        .iter()
        .find(|item| item.label == "limits")
        .expect("limits offered");
    let Some(CompletionTextEdit::Edit(edit)) = &limits.text_edit else {
        panic!("expected a replace edit");
    };
    let start = index.offset_of(text, edit.range.start, ENCODING);
    let end = index.offset_of(text, edit.range.end, ENCODING);
    assert_eq!(
        &text[start..end],
        "[lim",
        "the edit covers the typed bracket"
    );
    assert_eq!(edit.new_text, "[limits]");
}

#[test]
fn json_completion_absorbs_the_opening_quote() {
    // Arrange
    // A half-typed `"por` does not parse. Completing `port` replaces the quote
    // and the prefix, so the result is `"port": ` rather than a doubled quote.
    let text = "{\n  \"por\n}\n";
    let offset = text.find("\"por").unwrap() + "\"por".len();
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Json,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let port = items
        .iter()
        .find(|item| item.label == "port")
        .expect("the port field is offered");
    assert_eq!(inserted(port), "\"port\": ");
    let range = match &port.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit.range,
        other => panic!("an explicit replace edit, got {other:?}"),
    };
    // The edit starts at the opening quote (character 2 on line 1), not after it.
    assert_eq!(
        range.start,
        Position {
            line: 1,
            character: 2
        }
    );
}

#[test]
fn json_member_insert_is_non_destructive() {
    // Arrange
    // Completing a new key on a fresh line inside a populated object inserts the
    // member alone, with no comma, as a zero-width edit at the cursor.
    let text = "{\n  \"hostname\": \"api\"\n  \n}\n";
    let offset = text.find("\n  \n}").unwrap() + "\n  ".len();
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Json,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let port = items
        .iter()
        .find(|item| item.label == "port")
        .expect("the port field is offered");
    assert_eq!(inserted(port), "\"port\": ");
    let range = match &port.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit.range,
        other => panic!("an explicit replace edit, got {other:?}"),
    };
    assert_eq!(range.start, range.end, "the member insert is zero-width");
}

#[test]
fn json_enum_value_completion_offers_the_allowed_strings() {
    // Arrange
    let text = "{ \"limits\": { \"mode\": \"l\" } }\n";
    let offset = text.find("\"l\"").unwrap() + 2;
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Json,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let mut labels = labels(&items);
    labels.sort();
    assert_eq!(labels, vec!["enforce", "log", "off"]);
}

#[test]
fn json_repeated_block_completion_opens_an_array() {
    // Arrange
    let text = "{\n  \n}";
    let offset = text.find("\n  \n").unwrap() + "\n  ".len();
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Json,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let rules = items
        .iter()
        .find(|item| item.label == "rules")
        .expect("the rules block is offered");
    assert_eq!(inserted(rules), "\"rules\": [{  }]");
}

#[test]
fn json_field_in_an_array_element_position_opens_an_object() {
    // Arrange
    // The cursor is directly in the rules array, after the first element, so a
    // field completes as a new object element rather than a bare member.
    let text = "{\n  \"rules\": [\n    { \"prefix\": \"/api\" },\n    \n  ]\n}\n";
    let offset = text.find("},\n    \n").unwrap() + "},\n    ".len();
    assert!(
        Json.parse_tree(text).is_none(),
        "the trailing comma does not parse"
    );
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let plain = completion(
        &Json,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );
    let snippet = completion(
        &Json,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport {
            snippets: true,
            preselect: false,
        },
    );

    // Assert
    let prefix = plain
        .iter()
        .find(|item| item.label == "prefix")
        .expect("the prefix field is offered");
    assert_eq!(inserted(prefix), "{ \"prefix\":  }");
    // A snippet-capable client receives a `$0` tab stop inside the braces, so
    // the cursor lands at the value rather than after the closing brace.
    let prefix_snippet = snippet
        .iter()
        .find(|item| item.label == "prefix")
        .expect("the prefix field is offered");
    assert_eq!(inserted(prefix_snippet), "{ \"prefix\": $0 }");
    assert_eq!(
        prefix_snippet.insert_text_format,
        Some(InsertTextFormat::SNIPPET)
    );
}

#[test]
fn json_list_and_map_completion_open_the_container() {
    // Arrange
    let text = "{\n  \n}";
    let offset = text.find("\n  \n").unwrap() + "\n  ".len();
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Json,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let allow = items
        .iter()
        .find(|i| i.label == "allow")
        .expect("allow offered");
    assert_eq!(inserted(allow), "\"allow\": []");
    let headers = items
        .iter()
        .find(|i| i.label == "headers")
        .expect("headers offered");
    assert_eq!(inserted(headers), "\"headers\": {  }");
}

#[test]
fn toml_list_and_map_completion_open_the_container() {
    // Arrange
    let text = "";
    let (tree, context) = at_with(&Toml, text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Toml,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    let allow = items
        .iter()
        .find(|i| i.label == "allow")
        .expect("allow offered");
    assert_eq!(inserted(allow), "allow = []");
    let headers = items
        .iter()
        .find(|i| i.label == "headers")
        .expect("headers offered");
    assert_eq!(inserted(headers), "headers = {  }");
}

/// The edit range of the first item offered at `offset`, resolved through the
/// real parse and cursor resolution rather than a synthesized position. `None`
/// when nothing is offered or the item carries no replace edit.
fn edit_range_at<F: Frontend>(frontend: &F, text: &str, offset: usize) -> Option<Range> {
    let (tree, context) = at_with(frontend, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();
    let items = completion(
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
    );
    match &items.first()?.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => Some(edit.range),
        _ => None,
    }
}

#[test]
fn toml_keyword_inside_a_list_replaces_only_that_element() {
    // Arrange
    let text = "modes = [\"log\", \"enf\"]\n";
    let offset = text.find("\"enf\"").expect("the element is present") + 2;

    // Act
    let range = edit_range_at(&Toml, text, offset).expect("a replace edit is offered");

    // Assert
    assert_eq!(range.start.character, 16);
    assert_eq!(range.end.character, 21);
}

#[test]
fn json_keyword_inside_a_list_replaces_only_that_element() {
    // Arrange
    let text = "{\n  \"modes\": [\"log\", \"enf\"]\n}\n";
    let offset = text.find("\"enf\"").expect("the element is present") + 2;

    // Act
    let range = edit_range_at(&Json, text, offset).expect("a replace edit is offered");

    // Assert
    // Line 1 is the member line. The range covers the element alone, so the
    // brackets and the sibling entry survive the edit.
    assert_eq!(range.start.line, 1);
    assert_eq!(range.start.character, 19);
    assert_eq!(range.end.character, 24);
}
