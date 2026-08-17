//! The completion handler for the HCL and KDL frontends: unset fields and
//! nested blocks at the root, enum values, snippet and plain block inserts,
//! replace ranges for half-typed names and values, and declaration-order
//! sorting.

mod fixture;
mod support;

use lsp_types::{CompletionItemKind, CompletionTextEdit, InsertTextFormat, Position, Range};

use confval::schema::ToSchema;
use confval_lsp::handlers::{ClientSupport, Cx, completion};
use confval_lsp::{Frontend, Hcl, Kdl, LineIndex};

use fixture::ServerSpec;
use support::{ENCODING, at, at_with, inserted, labels};

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
    assert_eq!(limits.kind, Some(CompletionItemKind::STRUCT));
}

#[test]
fn block_completion_places_the_cursor_with_a_snippet_when_supported() {
    // Arrange
    // A block insert carries a `$0` tab stop. A snippet-capable client receives
    // it as a snippet so the cursor lands in the body. A client without snippet
    // support receives the plain text with the tab stop removed.
    let text = "";
    let (tree, context) = at(text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let with = completion(
        &Hcl,
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
    let without = completion(
        &Hcl,
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
    let snippet = with
        .iter()
        .find(|item| item.label == "limits")
        .expect("the limits block is offered");
    assert_eq!(inserted(snippet), "limits {\n  $0\n}");
    assert_eq!(snippet.insert_text_format, Some(InsertTextFormat::SNIPPET));
    let plain = without
        .iter()
        .find(|item| item.label == "limits")
        .expect("the limits block is offered");
    assert_eq!(inserted(plain), "limits {\n  \n}");
    assert_eq!(plain.insert_text_format, None);
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
    assert!(
        items
            .iter()
            .all(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER))
    );
}

#[test]
fn kdl_value_completion_on_a_valueless_node_does_not_replace_the_name() {
    // Arrange
    // A bare KDL node parses as an empty argument list whose span covers the
    // node name. Completing its value must insert at the cursor, so the edit is
    // zero-width and `mode ` becomes `mode "enforce"` rather than `"enforce"`.
    let text = "limits {\n  mode \n}\n";
    let offset = text.find("mode ").unwrap() + "mode ".len();
    let tree = Kdl.parse_tree(text);
    let context = Kdl.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Kdl,
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
        .expect("the enforce keyword is offered");
    assert_eq!(inserted(enforce), "\"enforce\"");
    let range = match &enforce.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit.range,
        other => panic!("an explicit replace edit, got {other:?}"),
    };
    assert_eq!(
        range.start, range.end,
        "the edit is a zero-width insert, so it never overwrites the node name"
    );
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
        items.is_empty(),
        "a map has open keys, so its body offers nothing"
    );
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
        .expect("port offered");
    assert_eq!(inserted(port), "port ");
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
            assert_eq!(edit.new_text, "workers = 4", "the default pre-fills");
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
fn enum_completion_works_at_an_empty_value_when_the_buffer_does_not_parse() {
    // Arrange
    // `mode = ` with an empty value does not parse. Recovery from the raw text
    // must still place the cursor at mode's value inside limits and offer the
    // enum, rather than the root fields.
    let text = "limits {\n  max_body_mb = 10\n  mode = \n}\n";
    let offset = text.find("mode = ").unwrap() + "mode = ".len();
    let (tree, context) = at(text, offset);
    assert!(tree.is_none(), "the buffer does not parse");
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Hcl,
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
    let mode = items
        .iter()
        .find(|item| item.label == "mode")
        .expect("mode offered");
    let Some(CompletionTextEdit::Edit(edit)) = &mode.text_edit else {
        panic!("expected a replace edit");
    };
    assert_eq!(edit.range.start, edit.range.end, "a zero-width insert");
    assert_eq!(edit.new_text, "mode = \"enforce\"", "the default pre-fills");
}

#[test]
fn value_completion_at_a_non_keyword_field_offers_nothing() {
    // Arrange
    let text = "port = 8080\n";
    let offset = text.find("8080").unwrap() + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Hcl,
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
        items.is_empty(),
        "a range field's value offers no completion"
    );
}

#[test]
fn hcl_list_and_map_completion_open_the_container() {
    // Arrange
    let text = "";
    let (tree, context) = at_with(&Hcl, text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Hcl,
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

#[test]
fn completion_sorts_by_schema_declaration_order() {
    // Arrange
    let text = "";
    let (tree, context) = at(text, 0);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let items = completion(
        &Hcl,
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
    let sort_keys: Vec<String> = items
        .iter()
        .map(|item| item.sort_text.clone().expect("a sort text"))
        .collect();
    let mut sorted = sort_keys.clone();
    sorted.sort();
    assert_eq!(
        sort_keys, sorted,
        "declaration order survives a client sort"
    );
    assert_eq!(items[0].label, "hostname", "the first declared field leads");
}
