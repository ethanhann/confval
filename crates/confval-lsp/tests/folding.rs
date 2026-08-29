//! Folding ranges against the fixtures: one line range per declared block
//! instance, in each of the five formats.

mod fixture;

use confval::schema::ToSchema;
use confval_lsp::handlers::folding_ranges;
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};

use fixture::{GatewaySpec, MeshSpec, ServerSpec};

const ENCODING: PositionEncoding = PositionEncoding::Utf8;

/// Parses a document and answers the folds as `(start_line, end_line)` pairs.
fn folds<F: Frontend>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
) -> Vec<(u32, u32)> {
    let Some(tree) = frontend.parse_tree(text) else {
        return Vec::new();
    };
    let index = LineIndex::new(text);
    folding_ranges(
        schema,
        &tree,
        text,
        frontend.block_span_covers_body(),
        frontend.recovery(),
        &index,
        ENCODING,
    )
    .iter()
    .map(|range| (range.start_line, range.end_line))
    .collect()
}

#[test]
fn hcl_blocks_fold_from_the_header_to_the_closing_brace() {
    // Arrange
    let text = "hostname = \"h\"\nport = 1\n\nlimits {\n  max_body_mb = 16\n  mode = \"enforce\"\n}\n\nrules {\n  prefix = \"/a\"\n}\n\nrules {\n  prefix = \"/b\"\n}\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Hcl, &schema, text);

    // Assert
    assert_eq!(folds, vec![(3, 6), (8, 10), (12, 14)]);
}

#[test]
fn kdl_blocks_fold_from_the_node_to_the_closing_brace() {
    // Arrange
    let text = "hostname \"h\"\nport 1\nlimits {\n  max_body_mb 16\n}\nrules {\n  prefix \"/a\"\n}\nrules {\n  prefix \"/b\"\n}\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Kdl, &schema, text);

    // Assert
    assert_eq!(folds, vec![(2, 4), (5, 7), (8, 10)]);
}

#[test]
fn json_objects_fold_from_the_key_to_the_closing_brace() {
    // Arrange
    let text = "{\n  \"hostname\": \"h\",\n  \"port\": 1,\n  \"limits\": {\n    \"max_body_mb\": 16\n  },\n  \"rules\": [\n    { \"prefix\": \"/a\" },\n    {\n      \"prefix\": \"/b\"\n    }\n  ]\n}\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Json, &schema, text);

    // Assert
    assert_eq!(folds, vec![(3, 5), (8, 10)]);
}

#[test]
fn toml_tables_fold_to_their_last_value_and_not_to_a_following_comment() {
    // Arrange
    let text = "hostname = \"h\"\nport = 1\n\n[limits]\nmax_body_mb = 16\n\n# the rules\n[[rules]]\nprefix = \"/a\"\n\n[[rules]]\nprefix = \"/b\"\n\n# trailing\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Toml, &schema, text);

    // Assert
    assert_eq!(folds, vec![(3, 4), (7, 8), (10, 11)]);
}

#[test]
fn yaml_mappings_fold_to_their_last_child_and_not_to_a_following_comment() {
    // Arrange
    let text = "hostname: h\nport: 1\nlimits:\n  max_body_mb: 16\n  mode: enforce\n\n# the rules\nrules:\n  - prefix: /a\n  - prefix:\n      /b\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Yaml, &schema, text);

    // Assert
    assert_eq!(folds, vec![(2, 4), (9, 10)]);
}

#[test]
fn yaml_sequence_elements_fold_one_per_instance() {
    // Arrange
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\n  - name: web\n    host: h2\n    port: 2\nroutes:\n  - prefix: /a\n    upstream: \"api\"\n  - prefix: /b\n    upstream: \"api\"\n";
    let schema = GatewaySpec::schema();

    // Act
    let folds = folds(&Yaml, &schema, text);

    // Assert
    assert_eq!(folds, vec![(1, 3), (4, 6), (8, 9), (10, 11)]);
}

/// Parses a document and answers each fold's end line and end character.
fn fold_ends<F: Frontend>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
) -> Vec<(u32, Option<u32>)> {
    let Some(tree) = frontend.parse_tree(text) else {
        return Vec::new();
    };
    let index = LineIndex::new(text);
    folding_ranges(
        schema,
        &tree,
        text,
        frontend.block_span_covers_body(),
        frontend.recovery(),
        &index,
        ENCODING,
    )
    .iter()
    .map(|range| (range.end_line, range.end_character))
    .collect()
}

#[test]
fn a_brace_fold_ends_at_the_closing_brace_column() {
    // Arrange
    let hcl = "hostname = \"h\"\nport = 1\nlimits {\n  max_body_mb = 16\n}\nrules {\n  prefix = \"/a\"\n}\n";
    let json = "{\n  \"hostname\": \"h\",\n  \"port\": 1,\n  \"limits\": {\n    \"max_body_mb\": 16\n  },\n  \"rules\": []\n}\n";
    let toml = "hostname = \"h\"\nport = 1\n[limits]\nmax_body_mb = 16\n";
    let schema = ServerSpec::schema();

    // Act
    let hcl_ends = fold_ends(&Hcl, &schema, hcl);
    let json_ends = fold_ends(&Json, &schema, json);
    let toml_ends = fold_ends(&Toml, &schema, toml);

    // Assert
    assert_eq!(hcl_ends, vec![(4, Some(0)), (7, Some(0))]);
    assert_eq!(json_ends, vec![(5, Some(2))]);
    assert_eq!(toml_ends, vec![(3, Some("max_body_mb = 16".len() as u32))]);
}

#[test]
fn hcl_labeled_repeats_fold_one_per_instance() {
    // Arrange
    let text = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nupstream \"web\" {\n  host = \"h2\"\n  port = 2\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";
    let schema = GatewaySpec::schema();

    // Act
    let folds = folds(&Hcl, &schema, text);

    // Assert
    assert_eq!(folds, vec![(0, 3), (4, 7), (8, 11)]);
}

#[test]
fn a_yaml_block_ending_in_an_empty_flow_map_folds_to_that_line() {
    // Arrange
    let text = "services:\n  - name: a\n    routes:\n      - upstream: u\n    upstreams: {}\n";
    let schema = MeshSpec::schema();

    // Act
    let folds = folds(&Yaml, &schema, text);

    // Assert
    assert_eq!(folds, vec![(1, 4)]);
}

#[test]
fn nested_blocks_fold_at_every_level() {
    // Arrange
    let text = "services {\n  name = \"a\"\n  upstreams \"u\" {\n    port = 1\n  }\n}\n";
    let schema = MeshSpec::schema();

    // Act
    let folds = folds(&Hcl, &schema, text);

    // Assert
    assert_eq!(folds, vec![(0, 5), (2, 4)]);
}

#[test]
fn a_string_map_does_not_fold() {
    // Arrange
    let text = "hostname = \"h\"\nport = 1\nheaders = {\n  a = \"b\"\n  c = \"d\"\n}\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Hcl, &schema, text);

    // Assert
    assert!(folds.is_empty());
}

#[test]
fn a_single_line_block_produces_no_range() {
    // Arrange
    let text = "hostname = \"h\"\nport = 1\nlimits { max_body_mb = 16 }\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Hcl, &schema, text);

    // Assert
    assert!(folds.is_empty());
}

#[test]
fn crlf_line_endings_keep_the_end_line_on_the_closing_brace() {
    // Arrange
    let text = "hostname = \"h\"\r\nport = 1\r\nlimits {\r\n  max_body_mb = 16\r\n}\r\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Hcl, &schema, text);

    // Assert
    assert_eq!(folds, vec![(2, 4)]);
}

#[test]
fn a_buffer_that_does_not_parse_answers_empty() {
    // Arrange
    let text = "hostname = \nlimits {\n";
    let schema = ServerSpec::schema();

    // Act
    let folds = folds(&Hcl, &schema, text);

    // Assert
    assert!(folds.is_empty());
}
