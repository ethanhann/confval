//! The pure handlers, tested directly against the fixture.

mod fixture;

use std::str::FromStr;

use lsp_types::{
    CompletionItemKind, CompletionTextEdit, DiagnosticSeverity, HoverContents, InsertTextFormat,
    Position, Range, Uri,
};

use confval::prelude::{Located, Report, Validate};
use confval::schema::ToSchema;
use confval_lsp::handlers::{completion, diagnostics, hover};
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};

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
        false,
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
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        true,
    );
    let without = completion(
        &Hcl,
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
    assert!(value.contains("Has a default."), "got: {value}");
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        false,
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
        false,
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
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        false,
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
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
    let found = diagnostics::<PlainSpec, Hcl>(&Hcl, text, &uri, ENCODING);

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
    let found = diagnostics::<ServerSpec, Json>(&Json, text, &uri, ENCODING);

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
    let found = diagnostics::<ServerSpec, Json>(&Json, text, &uri, ENCODING);

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
    let found = diagnostics::<ServerSpec, Yaml>(&Yaml, text, &uri, ENCODING);

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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
        &schema,
        tree.as_ref(),
        &context,
        text,
        &index,
        ENCODING,
        false,
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
