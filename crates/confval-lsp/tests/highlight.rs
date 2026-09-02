//! Document highlight against the fixtures: the label as a write occurrence
//! and each resolving reference as a read occurrence.

mod fixture;

use lsp_types::{DocumentHighlight, DocumentHighlightKind};

use confval::schema::ToSchema;
use confval_lsp::handlers::{Cx, document_highlight};
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};

use fixture::GatewaySpec;

const ENCODING: PositionEncoding = PositionEncoding::Utf8;

const GATEWAY_HCL: &str = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\nroutes {\n  prefix = \"/b\"\n  upstream = \"api\"\n}\n";

const GATEWAY_YAML: &str = "upstream:\n  - name: api\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: \"api\"\n  - prefix: /b\n    upstream: \"api\"\n";

/// Resolves a cursor, runs the handler, and answers each highlight as its
/// kind and the text it covers.
fn highlights_at<F: Frontend>(
    frontend: &F,
    text: &str,
    offset: usize,
) -> Vec<(DocumentHighlightKind, String, u32)> {
    let schema = GatewaySpec::schema();
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let index = LineIndex::new(text);
    let cx = Cx {
        schema: &schema,
        fields: tree.as_ref(),
        ctx: &context,
        text,
    };
    document_highlight(&cx, &index, ENCODING)
        .iter()
        .map(|highlight| {
            (
                kind(highlight),
                covered(highlight, text, &index),
                highlight.range.start.line,
            )
        })
        .collect()
}

fn kind(highlight: &DocumentHighlight) -> DocumentHighlightKind {
    highlight.kind.unwrap_or(DocumentHighlightKind::TEXT)
}

fn covered(highlight: &DocumentHighlight, text: &str, index: &LineIndex) -> String {
    let start = index.offset_of(text, highlight.range.start, ENCODING);
    let end = index.offset_of(text, highlight.range.end, ENCODING);
    text[start..end].to_string()
}

#[test]
fn a_native_label_cursor_highlights_the_label_and_its_references() {
    // Arrange
    let offset = GATEWAY_HCL.find("\"api\"").unwrap() + 1;

    // Act
    let highlights = highlights_at(&Hcl, GATEWAY_HCL, offset);

    // Assert
    assert_eq!(
        highlights,
        vec![
            (DocumentHighlightKind::WRITE, "api".to_string(), 0),
            (DocumentHighlightKind::READ, "api".to_string(), 6),
            (DocumentHighlightKind::READ, "api".to_string(), 10),
        ]
    );
}

#[test]
fn a_reference_cursor_highlights_the_same_set() {
    // Arrange
    let hcl_offset = GATEWAY_HCL.rfind("\"api\"").unwrap() + 1;
    let yaml_offset = GATEWAY_YAML.rfind("\"api\"").unwrap() + 1;

    // Act
    let hcl = highlights_at(&Hcl, GATEWAY_HCL, hcl_offset);
    let yaml = highlights_at(&Yaml, GATEWAY_YAML, yaml_offset);

    // Assert
    assert_eq!(
        hcl,
        vec![
            (DocumentHighlightKind::WRITE, "api".to_string(), 0),
            (DocumentHighlightKind::READ, "api".to_string(), 6),
            (DocumentHighlightKind::READ, "api".to_string(), 10),
        ]
    );
    assert_eq!(
        yaml,
        vec![
            (DocumentHighlightKind::WRITE, "api".to_string(), 1),
            (DocumentHighlightKind::READ, "api".to_string(), 6),
            (DocumentHighlightKind::READ, "api".to_string(), 8),
        ]
    );
}

#[test]
fn every_quoted_format_highlights_from_the_label_and_from_a_reference() {
    // Arrange
    let kdl = "upstream \"api\" {\n  host \"h\"\n  port 1\n}\nroutes {\n  prefix \"/a\"\n  upstream \"api\"\n}\n";
    let toml = "[[upstream]]\nname = \"api\"\nhost = \"h\"\nport = 1\n\n[[routes]]\nprefix = \"/a\"\nupstream = \"api\"\n";
    let json = "{\n  \"upstream\": [{\"name\": \"api\", \"host\": \"h\", \"port\": 1}],\n  \"routes\": [{\"prefix\": \"/a\", \"upstream\": \"api\"}]\n}\n";

    // Act
    let sets = [
        (
            highlights_at(&Kdl, kdl, kdl.find("\"api\"").unwrap() + 1),
            0,
            6,
        ),
        (
            highlights_at(&Kdl, kdl, kdl.rfind("\"api\"").unwrap() + 1),
            0,
            6,
        ),
        (
            highlights_at(&Toml, toml, toml.find("\"api\"").unwrap() + 1),
            1,
            7,
        ),
        (
            highlights_at(&Toml, toml, toml.rfind("\"api\"").unwrap() + 1),
            1,
            7,
        ),
        (
            highlights_at(&Json, json, json.find(": \"api\"").unwrap() + 3),
            1,
            2,
        ),
        (
            highlights_at(&Json, json, json.rfind(": \"api\"").unwrap() + 3),
            1,
            2,
        ),
    ];

    // Assert
    for (highlights, label_line, reference_line) in sets {
        assert_eq!(
            highlights,
            vec![
                (DocumentHighlightKind::WRITE, "api".to_string(), label_line),
                (
                    DocumentHighlightKind::READ,
                    "api".to_string(),
                    reference_line
                ),
            ]
        );
    }
}

#[test]
fn a_designated_label_field_cursor_highlights_inside_the_quotes() {
    // Arrange
    let offset = GATEWAY_YAML.find("name: api").unwrap() + "name: a".len();

    // Act
    let highlights = highlights_at(&Yaml, GATEWAY_YAML, offset);

    // Assert
    assert_eq!(
        highlights,
        vec![
            (DocumentHighlightKind::WRITE, "api".to_string(), 1),
            (DocumentHighlightKind::READ, "api".to_string(), 6),
            (DocumentHighlightKind::READ, "api".to_string(), 8),
        ]
    );
}

#[test]
fn an_unresolved_reference_highlights_the_references_only() {
    // Arrange
    let text = GATEWAY_HCL.replace("upstream = \"api\"", "upstream = \"nope\"");
    let offset = text.rfind("\"nope\"").unwrap() + 1;

    // Act
    let highlights = highlights_at(&Hcl, &text, offset);

    // Assert
    assert_eq!(
        highlights,
        vec![
            (DocumentHighlightKind::READ, "nope".to_string(), 6),
            (DocumentHighlightKind::READ, "nope".to_string(), 10),
        ]
    );
}

#[test]
fn a_duplicate_label_scope_highlights_the_label_under_the_cursor() {
    // Arrange
    let text = format!("upstream \"api\" {{\n  host = \"x\"\n  port = 9\n}}\n{GATEWAY_HCL}");
    let offset = text.rfind("upstream \"api\"").unwrap() + "upstream \"a".len();

    // Act
    let highlights = highlights_at(&Hcl, &text, offset);

    // Assert
    assert_eq!(
        highlights,
        vec![
            (DocumentHighlightKind::WRITE, "api".to_string(), 4),
            (DocumentHighlightKind::READ, "api".to_string(), 10),
            (DocumentHighlightKind::READ, "api".to_string(), 14),
        ]
    );
}

#[test]
fn an_empty_label_and_a_label_field_name_highlight_nothing() {
    // Arrange
    let empty = GATEWAY_HCL.replace("upstream \"api\"", "upstream \"\"");
    let empty_offset = empty.find("\"\"").unwrap() + 1;
    let name_offset = GATEWAY_YAML.find("name: api").unwrap() + 1;

    // Act
    let on_empty = highlights_at(&Hcl, &empty, empty_offset);
    let on_name = highlights_at(&Yaml, GATEWAY_YAML, name_offset);

    // Assert
    assert!(on_empty.is_empty());
    assert!(on_name.is_empty());
}

#[test]
fn a_buffer_that_does_not_parse_highlights_nothing() {
    // Arrange
    let text = "upstream \"api\" {\n  host = \"h\"\nroutes {\n  upstream = \"api\"\n";
    let offset = text.find("\"api\"").unwrap() + 1;

    // Act
    let highlights = highlights_at(&Hcl, text, offset);

    // Assert
    assert!(highlights.is_empty());
}
