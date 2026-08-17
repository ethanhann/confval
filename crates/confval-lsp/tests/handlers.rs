//! The pure handlers, tested directly against the fixture.

mod fixture;

use std::str::FromStr;

use lsp_types::{
    CompletionItemKind, CompletionTextEdit, DiagnosticSeverity, HoverContents, InsertTextFormat,
    Position, Range, Uri,
};

use confval::prelude::{Located, Report, Validate};
use confval::schema::ToSchema;
use confval_lsp::handlers::{ClientSupport, Cx, completion, diagnostics, hover};
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};

use fixture::{GatewaySpec, ServerSpec};

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
    let found = diagnostics::<ServerSpec, Hcl>(&Hcl, &ServerSpec::schema(), text, &uri, ENCODING);

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
fn hover_omits_the_state_when_the_buffer_does_not_parse() {
    // Arrange
    // A half-typed name does not parse, so the set-versus-defaulted state is
    // unknown. The type and default flag still render, but the state line is
    // omitted rather than guessed as "not set".
    let text = "workers";
    let (tree, context) = at(text, text.len());
    assert!(tree.is_none(), "the buffer does not parse");
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING);

    // Assert
    let value = markdown(hover.expect("a hover for workers"));
    assert!(value.contains("Defaults to 4."), "got: {value}");
    assert!(!value.contains("Not set"), "the state is omitted: {value}");
    assert!(!value.contains("Set by the configuration"), "{value}");
}

#[test]
fn hover_states_a_declared_but_unset_field_is_defaulted() {
    // Arrange
    // The buffer parses. `workers` appears only in a comment, so it is declared
    // by the schema but absent from the parse, and hover reads it as defaulted.
    let text = "# workers\nport = 8080\n";
    let offset = text.find("workers").unwrap() + 1;
    let (tree, context) = at(text, offset);
    assert!(tree.is_some(), "the buffer parses");
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING);

    // Assert
    let value = markdown(hover.expect("a hover for workers"));
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
fn hover_on_a_value_renders_its_field() {
    // Arrange
    let text = "port = 8080\n";
    let offset = text.find("8080").unwrap() + 1;
    let (tree, context) = at(text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let hover = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING);

    // Assert
    let value = markdown(hover.expect("a hover on the value"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(value.contains("Set by the configuration."), "got: {value}");
}

#[test]
fn a_spanless_warning_maps_to_the_first_line_with_related_information() {
    // Arrange
    // A handwritten validator emits a warning with no primary span but a related
    // span. The diagnostic points at the first line, carries the Warning
    // severity, and keeps the related note.
    #[derive(confval::Spec)]
    struct PlainSpec {
        name: Located<String>,
    }
    impl Validate for PlainSpec {
        fn validate(&self, report: &mut Report) {
            report
                .warning("a general warning")
                .related(self.name.span, "declared here")
                .emit();
        }
    }
    let text = "name = \"api\"\n";
    let uri = Uri::from_str("file:///plain.hcl").unwrap();

    // Act
    let found = diagnostics::<PlainSpec, Hcl>(&Hcl, &PlainSpec::schema(), text, &uri, ENCODING);

    // Assert
    let warning = found
        .iter()
        .find(|diagnostic| diagnostic.message.contains("general warning"))
        .expect("a warning");
    assert_eq!(warning.severity, Some(DiagnosticSeverity::WARNING));
    assert_eq!(
        warning.range.start,
        Position {
            line: 0,
            character: 0
        }
    );
    let related = warning
        .related_information
        .as_ref()
        .expect("related information");
    assert!(related.iter().any(|note| note.message == "declared here"));
}

/// The Markdown body of a hover.
fn markdown(hover: lsp_types::Hover) -> String {
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        other => panic!("expected markup hover, got: {other:?}"),
    }
}

/// Resolves a cursor against any frontend, for the JSON and YAML handler tests.
fn at_with<F: Frontend>(
    frontend: &F,
    text: &str,
    offset: usize,
) -> (Option<confval::format::Fields>, confval_lsp::CursorContext) {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    (tree, context)
}

#[test]
fn json_diagnostics_report_the_pipeline_issues() {
    // Arrange
    let text = "{\n  \"hostname\": \"api\",\n  \"port\": 99999,\n  \"bogus\": 1,\n  \"limits\": { \"mode\": \"nope\" }\n}\n";
    let uri = Uri::from_str("file:///fixture.json").unwrap();

    // Act
    let found = diagnostics::<ServerSpec, Json>(&Json, &ServerSpec::schema(), text, &uri, ENCODING);

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
}

#[test]
fn json_root_not_an_object_reports_a_parse_error() {
    // Arrange
    // A JSON array root cannot hold named fields, so the pipeline reports it.
    let text = "[]\n";
    let uri = Uri::from_str("file:///fixture.json").unwrap();

    // Act
    let found = diagnostics::<ServerSpec, Json>(&Json, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    assert!(
        found
            .iter()
            .any(|d| d.message.contains("object at the document root")),
        "expected a root parse error, got: {found:?}"
    );
}

#[test]
fn yaml_diagnostics_report_the_pipeline_issues() {
    // Arrange
    let text = "hostname: api\nport: 99999\nbogus: 1\nlimits:\n  mode: nope\n";
    let uri = Uri::from_str("file:///fixture.yaml").unwrap();

    // Act
    let found = diagnostics::<ServerSpec, Yaml>(&Yaml, &ServerSpec::schema(), text, &uri, ENCODING);

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
fn yaml_hover_renders_the_field_under_the_cursor() {
    // Arrange
    let text = "port: 8080\n";
    let offset = text.find("port").unwrap() + 1;
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let rendered = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING);

    // Assert
    let value = markdown(rendered.expect("a hover for port"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(value.contains("Between 1 and 65535"), "got: {value}");
}

#[test]
fn yaml_second_document_reports_a_parse_error() {
    // Arrange
    // A YAML stream with a second document cannot hold one configuration, so the
    // pipeline reports it.
    let text = "hostname: api\n---\nfoo: bar\n";
    let uri = Uri::from_str("file:///fixture.yaml").unwrap();

    // Act
    let found = diagnostics::<ServerSpec, Yaml>(&Yaml, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    assert!(
        found.iter().any(|d| d.message.contains("single document")),
        "expected a second-document parse error, got: {found:?}"
    );
}

#[test]
fn json_diagnostic_range_survives_a_non_ascii_earlier_value() {
    // Arrange
    // A non-ASCII value on an earlier line adds bytes; the port diagnostic on a
    // later line must still map to the right line and column.
    let text = "{\n  \"hostname\": \"café\",\n  \"port\": 99999\n}\n";
    let uri = Uri::from_str("file:///fixture.json").unwrap();

    // Act
    let found = diagnostics::<ServerSpec, Json>(&Json, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    let port = found
        .iter()
        .find(|d| d.message.contains("port"))
        .expect("a port diagnostic");
    assert_eq!(
        port.range.start,
        Position {
            line: 2,
            character: 10
        }
    );
}

#[test]
fn yaml_diagnostic_range_survives_a_non_ascii_earlier_value() {
    // Arrange
    let text = "hostname: café\nport: 99999\n";
    let uri = Uri::from_str("file:///fixture.yaml").unwrap();

    // Act
    let found = diagnostics::<ServerSpec, Yaml>(&Yaml, &ServerSpec::schema(), text, &uri, ENCODING);

    // Assert
    let port = found
        .iter()
        .find(|d| d.message.contains("port"))
        .expect("a port diagnostic");
    assert_eq!(
        port.range.start,
        Position {
            line: 1,
            character: 6
        }
    );
}

#[test]
fn json_hover_renders_the_field_under_the_cursor() {
    // Arrange
    let text = "{ \"port\": 8080 }\n";
    let offset = text.find("port").unwrap() + 1;
    let (tree, context) = at_with(&Json, text, offset);
    let index = LineIndex::new(text);
    let schema = ServerSpec::schema();

    // Act
    let rendered = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING);

    // Assert
    let value = markdown(rendered.expect("a hover for port"));
    assert!(value.contains("integer"), "got: {value}");
    assert!(value.contains("Between 1 and 65535"), "got: {value}");
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
    // The cursor sits directly in the rules array, after the first element, so a
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

/// The completion labels for the Gateway fixture at a cursor.
fn gateway_offered<F: Frontend>(frontend: &F, text: &str, offset: usize) -> Vec<String> {
    let (tree, context) = at_with(frontend, text, offset);
    let index = LineIndex::new(text);
    let schema = GatewaySpec::schema();
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
    labels(&items)
}

/// The hover markdown for the Gateway fixture at a cursor.
fn gateway_hover<F: Frontend>(frontend: &F, text: &str, offset: usize) -> String {
    let (tree, context) = at_with(frontend, text, offset);
    let index = LineIndex::new(text);
    let Some(hover) = hover(
        &GatewaySpec::schema(),
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
    ) else {
        panic!("a hover is produced");
    };
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("expected a markdown hover"),
    }
}

#[test]
fn completion_filters_the_already_set_fields_of_the_cursors_instance() {
    // Arrange
    // The first upstream sets host, the second sets port. A cursor on a blank
    // line inside the second offers host, unset there, and drops port, set there.
    // Reading the first instance would invert this.
    let hcl = "upstream \"a\" {\n  host = \"h\"\n}\nupstream \"b\" {\n  port = 8080\n  \n}\n";
    let hcl_off = hcl.find("port = 8080").unwrap() + "port = 8080\n  ".len();
    let kdl = "upstream \"a\" {\n  host \"h\"\n}\nupstream \"b\" {\n  port 8080\n  \n}\n";
    let kdl_off = kdl.find("port 8080").unwrap() + "port 8080\n  ".len();
    let toml =
        "[[upstream]]\nname = \"a\"\nhost = \"h\"\n\n[[upstream]]\nname = \"b\"\nport = 8080\n\n";
    let toml_off = toml.rfind("port = 8080").unwrap() + "port = 8080\n".len();
    let json = "{\n  \"upstream\": [\n    { \"name\": \"a\", \"host\": \"h\" },\n    {\n      \"name\": \"b\",\n      \"port\": 8080\n      \n    }\n  ]\n}\n";
    let json_off = json.find("\"port\": 8080").unwrap() + "\"port\": 8080\n      ".len();

    // Act
    let hcl_items = gateway_offered(&Hcl, hcl, hcl_off);
    let kdl_items = gateway_offered(&Kdl, kdl, kdl_off);
    let toml_items = gateway_offered(&Toml, toml, toml_off);
    let json_items = gateway_offered(&Json, json, json_off);

    // Assert
    for (format, items) in [
        ("hcl", &hcl_items),
        ("kdl", &kdl_items),
        ("toml", &toml_items),
        ("json", &json_items),
    ] {
        assert!(
            items.contains(&"host".to_string()),
            "{format} offers host, unset in the second instance: {items:?}"
        );
        assert!(
            !items.contains(&"port".to_string()),
            "{format} drops port, set in the second instance: {items:?}"
        );
    }
}

#[test]
fn hover_reads_the_state_from_the_cursors_instance() {
    // Arrange
    // Only the second upstream sets port. Hover on port in the second instance
    // reports it set. Reading the first instance would report it unset.
    let hcl = "upstream \"a\" {\n  host = \"h\"\n}\nupstream \"b\" {\n  host = \"h2\"\n  port = 8080\n}\n";
    let kdl =
        "upstream \"a\" {\n  host \"h\"\n}\nupstream \"b\" {\n  host \"h2\"\n  port 8080\n}\n";
    let toml = "[[upstream]]\nname = \"a\"\nhost = \"h\"\n\n[[upstream]]\nname = \"b\"\nhost = \"h2\"\nport = 8080\n";
    let json = "{\n  \"upstream\": [\n    { \"name\": \"a\", \"host\": \"h\" },\n    { \"name\": \"b\", \"host\": \"h2\", \"port\": 8080 }\n  ]\n}\n";
    let yaml =
        "upstream:\n  - name: a\n    host: alpha\n  - name: b\n    host: beta\n    port: 8080\n";

    // Act
    let hcl_hover = gateway_hover(&Hcl, hcl, hcl.rfind("port").unwrap() + 1);
    let kdl_hover = gateway_hover(&Kdl, kdl, kdl.rfind("port").unwrap() + 1);
    let toml_hover = gateway_hover(&Toml, toml, toml.rfind("port").unwrap() + 1);
    let json_hover = gateway_hover(&Json, json, json.rfind("port").unwrap() + 1);
    let yaml_hover = gateway_hover(&Yaml, yaml, yaml.rfind("port").unwrap() + 1);

    // Assert
    for (format, markdown) in [
        ("hcl", &hcl_hover),
        ("kdl", &kdl_hover),
        ("toml", &toml_hover),
        ("json", &json_hover),
        ("yaml", &yaml_hover),
    ] {
        assert!(
            markdown.contains("Set by the configuration."),
            "{format} reads port set in the second instance: {markdown:?}"
        );
    }
}

#[test]
fn reference_value_completion_offers_the_defined_labels() {
    // Arrange
    // Two upstreams are defined. The route's upstream reference value offers both
    // labels, collected from the whole document, in every format.
    let hcl = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nupstream \"web\" {\n  host = \"h2\"\n  port = 2\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"\"\n}\n";
    let hcl_off = hcl.rfind("upstream = \"").unwrap() + "upstream = \"".len();
    let kdl = "upstream \"api\" {\n  host \"h\"\n  port 1\n}\nupstream \"web\" {\n  host \"h2\"\n  port 2\n}\nroutes {\n  prefix \"/a\"\n  upstream \"\"\n}\n";
    let kdl_off = kdl.rfind("upstream \"").unwrap() + "upstream \"".len();
    let toml = "[[upstream]]\nname = \"api\"\nhost = \"h\"\nport = 1\n\n[[upstream]]\nname = \"web\"\nhost = \"h2\"\nport = 2\n\n[[routes]]\nprefix = \"/a\"\nupstream = \"\"\n";
    let toml_off = toml.rfind("upstream = \"").unwrap() + "upstream = \"".len();
    let json = "{\n  \"upstream\": [\n    { \"name\": \"api\", \"host\": \"h\", \"port\": 1 },\n    { \"name\": \"web\", \"host\": \"h2\", \"port\": 2 }\n  ],\n  \"routes\": [\n    { \"prefix\": \"/a\", \"upstream\": \"\" }\n  ]\n}\n";
    let json_off = json.rfind("\"upstream\": \"").unwrap() + "\"upstream\": \"".len();
    let yaml = "upstream:\n  - name: api\n    host: h\n    port: 1\n  - name: web\n    host: h2\n    port: 2\nroutes:\n  - prefix: /a\n    upstream: \"\"\n";
    let yaml_off = yaml.rfind("upstream: \"").unwrap() + "upstream: \"".len();

    // Act, Assert
    for (format, labels) in [
        ("hcl", gateway_offered(&Hcl, hcl, hcl_off)),
        ("kdl", gateway_offered(&Kdl, kdl, kdl_off)),
        ("toml", gateway_offered(&Toml, toml, toml_off)),
        ("json", gateway_offered(&Json, json, json_off)),
        ("yaml", gateway_offered(&Yaml, yaml, yaml_off)),
    ] {
        assert!(
            labels.contains(&"api".to_string()) && labels.contains(&"web".to_string()),
            "{format} offers the defined upstream labels: {labels:?}"
        );
    }
}

#[test]
fn a_cursor_in_a_block_label_offers_nothing_and_hovers_the_block() {
    // Arrange
    // A cursor inside the native label of an HCL or KDL block offers no
    // completion, because an author names a block freely, and hover names the
    // block the label belongs to.
    let hcl = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\n";
    let hcl_off = hcl.find("\"api\"").unwrap() + 1;
    let kdl = "upstream \"api\" {\n  host \"h\"\n  port 1\n}\n";
    let kdl_off = kdl.find("\"api\"").unwrap() + 1;

    // Act
    let hcl_items = gateway_offered(&Hcl, hcl, hcl_off);
    let hcl_hover = gateway_hover(&Hcl, hcl, hcl_off);
    let kdl_items = gateway_offered(&Kdl, kdl, kdl_off);
    let kdl_hover = gateway_hover(&Kdl, kdl, kdl_off);

    // Assert
    assert!(
        hcl_items.is_empty(),
        "hcl offers nothing in a label: {hcl_items:?}"
    );
    assert!(
        hcl_hover.contains("Label of the `upstream` block."),
        "hcl hover names the block: {hcl_hover:?}"
    );
    assert!(
        kdl_items.is_empty(),
        "kdl offers nothing in a label: {kdl_items:?}"
    );
    assert!(
        kdl_hover.contains("Label of the `upstream` block."),
        "kdl hover names the block: {kdl_hover:?}"
    );
}

#[test]
fn hover_on_a_reference_value_states_the_target_and_resolution() {
    // Arrange
    // A resolved reference names its target and says it resolves. An undefined
    // reference names the target and says it does not.
    let resolved = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";
    let resolved_off = resolved.rfind("upstream = \"api\"").unwrap() + "upstream = \"".len();
    let unresolved = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"nope\"\n}\n";
    let unresolved_off = unresolved.rfind("upstream = \"nope\"").unwrap() + "upstream = \"".len();

    // Act
    let resolved_hover = gateway_hover(&Hcl, resolved, resolved_off);
    let unresolved_hover = gateway_hover(&Hcl, unresolved, unresolved_off);

    // Assert
    assert!(
        resolved_hover.contains("References the `upstream` block."),
        "names the target: {resolved_hover:?}"
    );
    assert!(
        resolved_hover.contains("Resolves to a defined label."),
        "reports resolution: {resolved_hover:?}"
    );
    assert!(
        unresolved_hover.contains("Does not resolve to any defined label."),
        "reports a miss: {unresolved_hover:?}"
    );
}

#[test]
fn reference_hover_reports_unknown_resolution_without_a_parse() {
    // Arrange
    // An unterminated value does not parse, so hover names the target but cannot
    // say whether the value resolves.
    let text = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"ap";

    // Act
    let markdown = gateway_hover(&Hcl, text, text.len());

    // Assert
    assert!(
        markdown.contains("References the `upstream` block."),
        "names the target: {markdown:?}"
    );
    assert!(
        markdown.contains("Resolution is unknown"),
        "reports unknown resolution: {markdown:?}"
    );
}

#[test]
fn reference_completion_offers_nothing_without_a_parse() {
    // Arrange
    // An unterminated value does not parse, so the reference arm has no labels to
    // collect and offers nothing.
    let text = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"ap";
    let (tree, context) = at_with(&Hcl, text, text.len());
    let index = LineIndex::new(text);

    // Act
    let items = completion(
        &Hcl,
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
    assert!(tree.is_none(), "the buffer does not parse");
    assert!(
        items.is_empty(),
        "no labels are offered without a parse: {:?}",
        labels(&items)
    );
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
fn a_type_error_in_one_element_does_not_diagnose_a_sibling_element() {
    // Arrange
    // An invalid port in the first upstream element is the only diagnostic. The
    // valid port in the second element is not flagged, so a parse failure in one
    // instance does not contaminate a sibling.
    let text = "upstream:\n  - name: c\n    host: c.internal\n    port: assd\n  - name: b\n    host: b.internal\n    port: 8081\n";
    let uri = Uri::from_str("file:///g.yaml").unwrap();

    // Act
    let found =
        diagnostics::<GatewaySpec, Yaml>(&Yaml, &GatewaySpec::schema(), text, &uri, ENCODING);

    // Assert
    assert_eq!(found.len(), 1, "one diagnostic: {found:?}");
    assert_eq!(found[0].message, "expected integer, found string");
    assert_eq!(
        found[0].range.start.line, 3,
        "on the invalid port, not the valid one"
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

/// The mesh document the scoped editor tests share: two services, each with
/// its own labeled upstream and a route naming it.
const MESH_YAML: &str = "services:\n  - name: a\n    upstreams:\n      - name: ua\n        port: 1\n  - name: b\n    upstreams:\n      - name: ub\n        port: 2\n    routes:\n      - upstream: \"ub\"\n";

#[test]
fn scoped_reference_completion_offers_only_the_own_scope_labels() {
    // Arrange
    // The cursor sits in service b's route value. The declaring scope is that
    // service, so only its `ub` label is offered, not the sibling's `ua`.
    let text = MESH_YAML;
    let offset = text.rfind("ub").unwrap();
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
    assert_eq!(labels(&items), vec!["ub".to_string()]);
}

#[test]
fn scoped_reference_hover_resolves_within_its_own_scope() {
    // Arrange
    let text = MESH_YAML;
    let offset = text.rfind("ub").unwrap();
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = fixture::MeshSpec::schema();

    // Act
    let found = hover(&schema, tree.as_ref(), &context, text, &index, ENCODING)
        .expect("a hover is produced");

    // Assert
    let markdown = match found.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("expected a markdown hover"),
    };
    assert!(
        markdown.contains("References the `upstreams` block."),
        "the target line names the block: {markdown}"
    );
    assert!(
        markdown.contains("Resolves to a defined label."),
        "the own-scope label resolves: {markdown}"
    );
}

#[test]
fn yaml_single_quoted_reference_value_hovers_as_resolved() {
    // Arrange
    // The parsed value of `'api'` is `api`, which the pipeline resolves. Hover
    // reads the parsed value from the resolved body, so it agrees.
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: 'api'\n";
    let offset = text.rfind("api").unwrap();

    // Act
    let markdown = gateway_hover(&Yaml, text, offset);

    // Assert
    assert!(
        markdown.contains("Resolves to a defined label."),
        "the single-quoted value resolves like diagnostics: {markdown}"
    );
}

#[test]
fn a_parsed_non_string_reference_value_hovers_without_a_resolution_line() {
    // Arrange
    // The reference pass skips a parsed non-string without a report, so hover
    // states the target and no resolution line.
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: 123\n";
    let offset = text.rfind("123").unwrap() + 1;

    // Act
    let markdown = gateway_hover(&Yaml, text, offset);

    // Assert
    assert!(
        markdown.contains("References the `upstream` block."),
        "the target line stays: {markdown}"
    );
    assert!(
        !markdown.to_lowercase().contains("resolve"),
        "no resolution claim for a value the pass skips: {markdown}"
    );
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

#[test]
fn hover_on_a_reference_field_name_states_the_target_block() {
    // Arrange
    // The field-name hover renders the constraint line, so a reference field
    // names its target rather than appending an empty section.
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: \"api\"\n";
    let offset = text.rfind("upstream:").unwrap() + 1;

    // Act
    let markdown = gateway_hover(&Yaml, text, offset);

    // Assert
    assert!(
        markdown.contains("**upstream**"),
        "the field hover renders: {markdown}"
    );
    assert!(
        markdown.contains("References the `upstream` block."),
        "the constraint line names the target: {markdown}"
    );
}

#[test]
fn yaml_reference_completion_after_a_bare_colon_keeps_the_colon() {
    // Arrange
    // The buffer parses, and `upstream:` holds a null. Accepting a label must
    // insert ` "a"` at the cursor, so the line becomes `upstream: "a"` rather
    // than the null's span swallowing the colon.
    let text = "upstream:\n  - name: a\n    host: h\n    port: 1\nroutes:\n  - prefix: /x\n    upstream:\n";
    let offset = text.rfind("upstream:").unwrap() + "upstream:".len();
    let (tree, context) = at_with(&Yaml, text, offset);
    assert!(tree.is_some(), "the buffer parses");
    let index = LineIndex::new(text);
    let schema = GatewaySpec::schema();

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
    let item = items.iter().find(|i| i.label == "a").expect("the label");
    let edit = match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit,
        other => panic!("a replace edit, got {other:?}"),
    };
    assert_eq!(edit.new_text, " \"a\"", "the insert supplies the space");
    assert_eq!(
        edit.range.start,
        Position {
            line: 6,
            character: 13
        }
    );
    assert_eq!(edit.range.start, edit.range.end, "zero width at the cursor");
}

#[test]
fn a_label_with_an_inner_quote_completes_escaped() {
    // Arrange
    // The label text enters the quoted insert, so its own quote must be
    // escaped rather than ending the literal early.
    let text = "upstream:\n  - name: 'a\"b'\n    host: h\n    port: 1\nroutes:\n  - prefix: /x\n    upstream: \n";
    let offset = text.rfind("upstream: ").unwrap() + "upstream: ".len();
    let (tree, context) = at_with(&Yaml, text, offset);
    let index = LineIndex::new(text);
    let schema = GatewaySpec::schema();

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
    let item = items.first().expect("the label");
    assert_eq!(inserted(item), "\"a\\\"b\"", "the inner quote is escaped");
}
