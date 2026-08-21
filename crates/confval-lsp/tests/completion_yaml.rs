//! The completion handler for the YAML frontend: mapping and scalar inserts,
//! sequence elements with their dash markers, indentation-aware bodies under
//! empty and pending keys, and edits after a bare colon.

mod fixture;
mod support;

use lsp_types::{CompletionTextEdit, Position, Range};

use confval::schema::ToSchema;
use confval_lsp::handlers::{ClientSupport, Cx, completion};
use confval_lsp::{LineIndex, Yaml};

use fixture::{GatewaySpec, ServerSpec};
use support::{ENCODING, at_with, inserted, labels};

#[test]
fn a_nested_yaml_block_insert_indents_its_body_to_the_cursor_column() {
    // Arrange
    // The completion inserts at column 4, so the sequence line below the key
    // must indent past that column. A client that applies the edit verbatim
    // would otherwise write the body outside the block.
    let text = "services:\n  - name: \"a\"\n    \n";
    let offset = text.len() - 1;
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = fixture::MeshSpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
    let item = items
        .iter()
        .find(|item| item.label == "upstreams")
        .unwrap_or_else(|| {
            panic!(
                "expected an upstreams item, got: {:?}",
                items.iter().map(|item| &item.label).collect::<Vec<_>>()
            )
        });
    let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
        panic!("expected a text edit");
    };
    assert_eq!(edit.new_text, "upstreams:\n      - ");
}

#[test]
fn yaml_completion_under_an_empty_key_offers_the_block_fields() {
    // Arrange
    // The `limits:` key awaits its body. A cursor on the indented line offers
    // the block's fields, proving the indentation resolution end to end.
    let text = "limits:\n  \n";
    let offset = text.len() - 1;
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
    let labels = labels(&items);
    assert!(
        labels.contains(&"mode".to_string()),
        "expected the limits block fields, got: {labels:?}"
    );
}

#[test]
fn yaml_completion_inserts_the_mapping_and_scalar_forms() {
    // Arrange
    let text = "";
    let (tree, context) = at_with(&Yaml, text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
        .expect("the limits block is offered");
    assert_eq!(inserted(limits), "limits:\n  ");
    let workers = items
        .iter()
        .find(|item| item.label == "workers")
        .expect("the workers field is offered");
    assert_eq!(inserted(workers), "workers: 4", "the default pre-fills");
}

#[test]
fn yaml_repeated_block_completion_opens_a_sequence() {
    // Arrange
    let text = "";
    let (tree, context) = at_with(&Yaml, text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
    assert_eq!(inserted(rules), "rules:\n  - ");
}

#[test]
fn yaml_field_in_a_repeated_block_opens_a_new_element() {
    // Arrange
    // A cursor on a fresh line inside the rules sequence completes a field as a
    // new element with a `-` marker, not a bare key.
    let text = "rules:\n  - prefix: \"/api\"\n  \n";
    let offset = text.len() - 1;
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
    let prefix = items
        .iter()
        .find(|item| item.label == "prefix")
        .expect("the prefix field is offered");
    assert_eq!(inserted(prefix), "- prefix: ");
}

#[test]
fn yaml_list_and_map_completion_open_the_container() {
    // Arrange
    let text = "";
    let (tree, context) = at_with(&Yaml, text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
    assert_eq!(inserted(allow), "allow:\n  - ");
    let headers = items
        .iter()
        .find(|i| i.label == "headers")
        .expect("headers offered");
    assert_eq!(inserted(headers), "headers:\n  ");
}

#[test]
fn yaml_field_inside_an_existing_element_adds_a_field_not_an_element() {
    // Arrange
    // The cursor is on a blank line at field indentation inside the first upstream
    // element. Completing a field adds it to that element without a dash marker,
    // and the fields already set in that element are dropped.
    let text = "upstream:\n  - name: a\n    host: a.internal\n    \n  - name: b\n    host: b.internal\n    port: 8081\n";
    let offset = text.find("host: a.internal\n    ").unwrap() + "host: a.internal\n    ".len();
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);

    // Act
    let items = completion(
        &Yaml,
        &Cx {
            schema: &GatewaySpec::schema(),
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );

    // Assert
    // Exactly one item, the unset `port`, because `name` and `host` are set in
    // the element and the server offers each field at most once.
    assert_eq!(labels(&items), vec!["port".to_string()]);
    assert_eq!(
        inserted(&items[0]),
        "port: ",
        "the field is added without a dash"
    );
}

#[test]
fn yaml_completion_under_a_pending_block_offers_every_block_field() {
    // Arrange
    // The root sets `port` and `admin:` awaits its body. The pending body sets
    // nothing, so the admin block's own `port` stays offered.
    let text = "port: 8080\nadmin:\n  \n";
    let offset = text.len() - 1;
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = fixture::RelaySpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
    let labels = labels(&items);
    assert!(
        labels.contains(&"port".to_string()),
        "expected the pending admin body to offer port, got: {labels:?}"
    );
}

#[test]
fn yaml_keyword_completion_after_a_bare_colon_keeps_the_key_and_adds_a_space() {
    // Arrange
    // The buffer does not parse, so the scanned token is the replace range. The
    // edit must start past the colon and the insert must supply the separating
    // space, so accepting a keyword yields `mode: "enforce"`.
    let text = "limits:\n  mode:\nbad: [\n";
    let offset = text.find("mode:").unwrap() + "mode:".len();
    let (tree, context) = at_with(&Yaml, text, offset);
    assert!(tree.is_none(), "the buffer does not parse");
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Yaml,
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
    let enforce = items
        .iter()
        .find(|item| item.label == "enforce")
        .expect("the enforce keyword");
    let edit = match &enforce.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit,
        other => panic!("a replace edit, got {other:?}"),
    };
    assert_eq!(
        edit.new_text, " \"enforce\"",
        "the insert supplies the space"
    );
    assert_eq!(
        edit.range.start,
        Position {
            line: 1,
            character: 7
        }
    );
    assert_eq!(
        edit.range.end,
        Position {
            line: 1,
            character: 7
        }
    );
}

#[test]
fn a_yaml_value_without_a_closed_set_offers_nothing() {
    // Arrange
    // The cursor sits inside a written scalar value in a nested sequence: once
    // in a label field and once in a plain string field. Neither field has a
    // closed value set, so the server must answer with an empty list rather
    // than field names, and the resolved context must still name the exact
    // field and path.
    let text = "services:\n  - name: checkout\n    upstreams:\n      - name: primary\n        port: 9000\n      - name: secondary\n        port: 9002\n";
    let schema = fixture::MeshSpec::schema();
    let index = LineIndex::new(text);
    let cases: Vec<(&str, &str, Vec<&str>)> = vec![
        ("a label value", "secondary", vec!["services", "upstreams"]),
        ("a plain string value", "checkout", vec!["services"]),
    ];

    for (name, needle, path) in cases {
        let offset = text.find(needle).expect("the needle") + 2;
        let (tree, context) = at_with(&Yaml, text, offset);

        // Act
        let items = completion(
            &Yaml,
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
        assert!(items.is_empty(), "case: {name}, got {:?}", labels(&items));
        assert_eq!(context.path, path, "case: {name}");
        assert_eq!(
            context.kind,
            confval_lsp::PositionKind::AttributeValue {
                field: "name".to_string()
            },
            "case: {name}"
        );
    }
}

/// The items and the first item's edit range at `offset`, resolved through the
/// real parse and cursor resolution.
fn offered_with_range(text: &str, offset: usize) -> (Vec<String>, Option<Range>) {
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();
    let items = completion(
        &Yaml,
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
    let range = items.first().and_then(|item| match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => Some(edit.range),
        _ => None,
    });
    (labels(&items), range)
}

#[test]
fn a_yaml_sequence_element_offers_the_list_keywords() {
    // Arrange
    // The frontend's own insert for a string list leaves the cursor here, on a
    // dash line under the key, so this is the position an operator reaches.
    let text = "modes:\n  - enf\n";
    let offset = text.find("enf").expect("the element is present") + 3;

    // Act
    let (offered, range) = offered_with_range(text, offset);

    // Assert
    assert_eq!(
        offered,
        vec!["enforce".to_string(), "log".to_string(), "off".to_string()]
    );
    let range = range.expect("a replace edit is offered");
    assert_eq!(range.start.line, 1);
    assert_eq!(range.start.character, 4);
    assert_eq!(range.end.character, 7);
}

#[test]
fn a_bare_yaml_sequence_dash_offers_the_list_keywords() {
    // Arrange
    let text = "modes:\n  - \n";
    let offset = text.find("- ").expect("the dash is present") + 2;

    // Act
    let (offered, _) = offered_with_range(text, offset);

    // Assert
    assert!(
        offered.contains(&"enforce".to_string()),
        "offered: {:?}",
        offered
    );
}

#[test]
fn a_yaml_block_sequence_still_completes_field_names() {
    // Arrange
    // `rules` is a repeated block, so its elements have a body. The list
    // redirect must not take this position.
    let text = "rules:\n  - \n";
    let offset = text.len() - 1;

    // Act
    let (offered, _) = offered_with_range(text, offset);

    // Assert
    assert!(
        offered.contains(&"prefix".to_string()),
        "offered: {:?}",
        offered
    );
}

/// The byte offset of an LSP position, for the ASCII fixtures these tests use.
fn byte_offset(text: &str, position: Position) -> usize {
    let line_start = text
        .split_inclusive('\n')
        .take(position.line as usize)
        .map(str::len)
        .sum::<usize>();
    line_start + position.character as usize
}

/// The text a completion at `offset` produces when applied to `text`, or
/// `None` when the item is absent or carries no replace edit.
fn applied(text: &str, offset: usize, label: &str) -> Option<String> {
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();
    let items = completion(
        &Yaml,
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
    let item = items.iter().find(|item| item.label == label)?;
    let (start, end) = match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => (
            byte_offset(text, edit.range.start),
            byte_offset(text, edit.range.end),
        ),
        _ => return None,
    };
    let new_text = inserted(item);
    Some(format!("{}{}{}", &text[..start], new_text, &text[end..]))
}

#[test]
fn accepting_a_keyword_in_a_quoted_yaml_element_does_not_double_the_quotes() {
    // Arrange
    let text = "modes:\n  - \"enf\"\n";
    let offset = text.find("enf").expect("the element is present") + 3;

    // Act
    let result = applied(text, offset, "enforce").expect("the item is offered");

    // Assert
    assert_eq!(result, "modes:\n  - \"enforce\"\n");
}
