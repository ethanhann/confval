//! Go-to-definition, find-references, and document symbols against the
//! fixtures, per the label-position table.

mod fixture;

use std::str::FromStr;

use lsp_types::{DocumentSymbolResponse, Location, SymbolKind, Uri};

use confval::schema::ToSchema;
use confval_lsp::handlers::{SymbolShape, definition, document_symbols, references};
use confval_lsp::{Frontend, Hcl, Json, Kdl, LineIndex, PositionEncoding, Toml, Yaml};

use fixture::{GatewaySpec, MeshSpec, ServerSpec};

const ENCODING: PositionEncoding = PositionEncoding::Utf8;

/// The test document URI.
fn doc_uri() -> Uri {
    match Uri::from_str("file:///doc") {
        Ok(uri) => uri,
        Err(_) => panic!("a valid uri"),
    }
}

/// The byte range a location covers, for asserting against the text.
fn covered(location: &Location, text: &str, index: &LineIndex) -> String {
    let start = index.offset_of(text, location.range.start, ENCODING);
    let end = index.offset_of(text, location.range.end, ENCODING);
    text[start..end].to_string()
}

/// Resolves a cursor and runs the definition handler.
fn definition_at<F: Frontend>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
    offset: usize,
) -> Option<Location> {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let uri = doc_uri();
    let index = LineIndex::new(text);
    definition(schema, &context, &uri, text, &index, ENCODING)
}

/// Resolves a cursor and runs the references handler.
fn references_at<F: Frontend>(
    frontend: &F,
    schema: &confval::schema::Schema,
    text: &str,
    offset: usize,
    include_declaration: bool,
) -> Vec<Location> {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    let uri = doc_uri();
    let index = LineIndex::new(text);
    references(
        schema,
        &context,
        include_declaration,
        &uri,
        text,
        &index,
        ENCODING,
    )
}

const GATEWAY_YAML: &str = "upstream:\n  - name: api\n    host: h\n    port: 1\n  - name: web\n    host: h2\n    port: 2\nroutes:\n  - prefix: /a\n    upstream: \"api\"\n  - prefix: /b\n    upstream: \"api\"\n";

const GATEWAY_HCL: &str = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\nroutes {\n  prefix = \"/b\"\n  upstream = \"api\"\n}\n";

#[test]
fn definition_on_a_reference_value_answers_the_label_span() {
    // Arrange
    let text = GATEWAY_YAML;
    let offset = text.find("upstream: \"api\"").unwrap() + "upstream: \"a".len();
    let schema = GatewaySpec::schema();
    let index = LineIndex::new(text);

    // Act
    let found = definition_at(&Yaml, &schema, text, offset).expect("a definition");

    // Assert
    let covered = covered(&found, text, &index);
    assert!(covered.contains("api"), "the label span, got {covered:?}");
    let label_offset = text.find("name: api").unwrap();
    let start = index.offset_of(text, found.range.start, ENCODING);
    assert!(
        start >= label_offset && start < label_offset + "name: api".len(),
        "the span sits on the first upstream's label"
    );
}

#[test]
fn definition_on_a_duplicated_label_answers_the_first_in_document_order() {
    // Arrange
    // Both upstreams carry the label `api`, which diagnostics already flag.
    // The pick is the first declaration, deterministically.
    let text = "upstream:\n  - name: api\n    host: h\n    port: 1\n  - name: api\n    host: h2\n    port: 2\nroutes:\n  - prefix: /a\n    upstream: \"api\"\n";
    let offset = text.rfind("\"api\"").unwrap() + 1;
    let schema = GatewaySpec::schema();
    let index = LineIndex::new(text);

    // Act
    let found = definition_at(&Yaml, &schema, text, offset).expect("a definition");

    // Assert
    let start = index.offset_of(text, found.range.start, ENCODING);
    assert!(
        start < text.find("- name: api\n    host: h2").unwrap(),
        "the first label wins"
    );
}

#[test]
fn definition_on_a_label_answers_empty() {
    // Arrange
    // The native HCL label is the definition, so definition answers nothing.
    let text = GATEWAY_HCL;
    let offset = text.find("\"api\"").unwrap() + 1;
    let schema = GatewaySpec::schema();

    // Act
    let found = definition_at(&Hcl, &schema, text, offset);

    // Assert
    assert!(found.is_none(), "a label is its own definition");
}

#[test]
fn definition_on_a_no_parse_buffer_answers_empty() {
    // Arrange
    let text = "routes:\n  - upstream: \"api\"\nbad: [\n";
    let offset = text.find("\"api\"").unwrap() + 1;
    let schema = GatewaySpec::schema();

    // Act
    let found = definition_at(&Yaml, &schema, text, offset);

    // Assert
    assert!(found.is_none(), "navigation needs a parse");
}

#[test]
fn references_from_a_native_label_list_the_reference_values() {
    // Arrange
    // The cursor is in the HCL block label. Both routes name it.
    let text = GATEWAY_HCL;
    let offset = text.find("\"api\"").unwrap() + 1;
    let schema = GatewaySpec::schema();
    let index = LineIndex::new(text);

    // Act
    let with = references_at(&Hcl, &schema, text, offset, true);
    let without = references_at(&Hcl, &schema, text, offset, false);

    // Assert
    assert_eq!(with.len(), 3, "the declaration and both references");
    assert_eq!(without.len(), 2, "both references without the declaration");
    for location in &without {
        assert!(
            covered(location, text, &index).contains("api"),
            "each hit covers a reference value"
        );
    }
}

#[test]
fn references_from_a_designated_label_field_list_the_reference_values() {
    // Arrange
    // The YAML label is the designated field's value, so the declaring scope
    // is the parent of the labeled block instance.
    let text = GATEWAY_YAML;
    let offset = text.find("name: api").unwrap() + "name: a".len();
    let schema = GatewaySpec::schema();

    // Act
    let found = references_at(&Yaml, &schema, text, offset, false);

    // Assert
    assert_eq!(found.len(), 2, "both routes name the label: {found:?}");
}

#[test]
fn references_from_a_reference_value_list_the_sibling_references() {
    // Arrange
    let text = GATEWAY_YAML;
    let offset = text.find("upstream: \"api\"").unwrap() + "upstream: \"a".len();
    let schema = GatewaySpec::schema();

    // Act
    let found = references_at(&Yaml, &schema, text, offset, true);

    // Assert
    assert_eq!(found.len(), 3, "the declaration and both references");
}

#[test]
fn references_stay_within_the_declaring_scope() {
    // Arrange
    // Two services each define upstream `u1`. From service a's label, only
    // service a's route appears.
    let text = "services:\n  - name: a\n    upstreams:\n      - name: u1\n        port: 1\n    routes:\n      - upstream: \"u1\"\n  - name: b\n    upstreams:\n      - name: u1\n        port: 2\n    routes:\n      - upstream: \"u1\"\n";
    let offset = text.find("name: u1").unwrap() + "name: u".len();
    let schema = MeshSpec::schema();
    let index = LineIndex::new(text);

    // Act
    let found = references_at(&Yaml, &schema, text, offset, false);

    // Assert
    assert_eq!(found.len(), 1, "only the own-scope route: {found:?}");
    let start = index.offset_of(text, found[0].range.start, ENCODING);
    assert!(
        start < text.find("- name: b").unwrap(),
        "the hit sits inside service a"
    );
}

#[test]
fn references_drop_a_site_resolving_to_a_nearer_scope() {
    // Arrange
    // The root and service both declare `pools`. The route's `shared` resolves
    // to the service's own pools, a nearer scope, so the root label's
    // references stay empty.
    let text = "pools:\n  - id: shared\nservices:\n  - name: a\n    pools:\n      - id: shared\n    routes:\n      - upstream: \"u\"\n        pool: \"shared\"\n    upstreams:\n      - name: u\n        port: 1\n";
    let offset = text.find("id: shared").unwrap() + "id: s".len();
    let schema = MeshSpec::schema();

    // Act
    let found = references_at(&Yaml, &schema, text, offset, false);

    // Assert
    assert!(
        found.is_empty(),
        "the shadowed root label has no references: {found:?}"
    );
}

#[test]
fn references_on_a_label_field_name_answer_empty() {
    // Arrange
    // The cursor is on the `name` key itself, a body position. Navigation
    // answers on the value, not the name.
    let text = GATEWAY_YAML;
    let offset = text.find("name: api").unwrap() + 1;
    let schema = GatewaySpec::schema();

    // Act
    let found = references_at(&Yaml, &schema, text, offset, true);

    // Assert
    assert!(found.is_empty());
}

#[test]
fn hierarchical_symbols_nest_the_blocks_with_their_labels() {
    // Arrange
    let text = GATEWAY_YAML;
    let schema = GatewaySpec::schema();
    let tree = Yaml.parse_tree(text).expect("the buffer parses");
    let uri = Uri::from_str("file:///doc").unwrap();
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(true, true),
        &uri,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let upstreams: Vec<_> = symbols.iter().filter(|s| s.name == "upstream").collect();
    assert_eq!(upstreams.len(), 2, "one container per instance");
    assert_eq!(upstreams[0].kind, SymbolKind::STRUCT);
    assert_eq!(upstreams[0].detail.as_deref(), Some("api"));
    assert_eq!(upstreams[1].detail.as_deref(), Some("web"));
    let children = upstreams[0].children.as_ref().expect("children");
    assert!(
        children
            .iter()
            .any(|c| c.name == "port" && c.kind == SymbolKind::FIELD),
        "the scalar children are leaf symbols"
    );
}

#[test]
fn each_repeated_instance_range_contains_its_own_children() {
    // Arrange
    // The `upstream` field has two instances followed by the `routes` field.
    // Each instance's range must bound its own children: the first ends at the
    // second instance's start, and the last ends at the following field.
    let text = GATEWAY_YAML;
    let schema = GatewaySpec::schema();
    let tree = Yaml.parse_tree(text).expect("the buffer parses");
    let uri = Uri::from_str("file:///doc").unwrap();
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(true, true),
        &uri,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let upstreams: Vec<_> = symbols.iter().filter(|s| s.name == "upstream").collect();
    assert_eq!(upstreams.len(), 2, "one container per instance");
    for upstream in &upstreams {
        let children = upstream.children.as_ref().expect("children");
        assert!(!children.is_empty(), "each instance has scalar children");
        for child in children {
            assert!(
                upstream.range.start <= child.range.start && child.range.end <= upstream.range.end,
                "child {:?} sits inside instance range {:?}",
                child.range,
                upstream.range
            );
        }
    }
    assert!(
        upstreams[0].range.end <= upstreams[1].range.start,
        "the first instance ends before the second begins"
    );
}

#[test]
fn a_leaf_selection_covers_the_field_name() {
    // Arrange
    // The `port` leaf name starts at the leaf range start, so the selection
    // must clamp to the name rather than collapse to a zero-width point.
    let text = GATEWAY_YAML;
    let schema = GatewaySpec::schema();
    let tree = Yaml.parse_tree(text).expect("the buffer parses");
    let uri = Uri::from_str("file:///doc").unwrap();
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(true, true),
        &uri,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let upstream = symbols
        .iter()
        .find(|s| s.name == "upstream")
        .expect("upstream");
    let children = upstream.children.as_ref().expect("children");
    let port = children
        .iter()
        .find(|c| c.name == "port")
        .expect("port leaf");
    let start = index.offset_of(text, port.selection_range.start, ENCODING);
    let end = index.offset_of(text, port.selection_range.end, ENCODING);
    assert_eq!(
        &text[start..end],
        "port",
        "the selection covers the field name"
    );
}

#[test]
fn toml_container_ranges_contain_their_children() {
    // Arrange
    // A TOML table's parsed span covers only its header, so the container's
    // range extends to keep its children inside it.
    let text = "hostname = \"h\"\nport = 1\n\n[limits]\nmax_body_mb = 64\nmode = \"enforce\"\n";
    let schema = ServerSpec::schema();
    let tree = Toml.parse_tree(text).expect("the buffer parses");
    let uri = Uri::from_str("file:///doc").unwrap();
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(Toml.block_span_covers_body(), true),
        &uri,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let limits = symbols.iter().find(|s| s.name == "limits").expect("limits");
    let children = limits.children.as_ref().expect("children");
    let mode = children.iter().find(|c| c.name == "mode").expect("mode");
    assert!(
        limits.range.start <= mode.range.start && mode.range.end <= limits.range.end,
        "the child range sits inside the container: {:?} in {:?}",
        mode.range,
        limits.range
    );
    assert!(
        limits.range.start <= limits.selection_range.start
            && limits.selection_range.end <= limits.range.end,
        "the selection sits inside the range"
    );
}

#[test]
fn the_flat_form_lists_every_symbol_with_its_container() {
    // Arrange
    let text = GATEWAY_YAML;
    let schema = GatewaySpec::schema();
    let tree = Yaml.parse_tree(text).expect("the buffer parses");
    let uri = Uri::from_str("file:///doc").unwrap();
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(true, false),
        &uri,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Flat(symbols) = response else {
        panic!("the flat form");
    };
    let port = symbols
        .iter()
        .find(|s| s.name == "port")
        .expect("a nested field");
    assert_eq!(port.container_name.as_deref(), Some("upstream"));
}

#[test]
fn symbol_kinds_cover_the_field_shapes() {
    // Arrange
    let text = "hostname: h\nport: 1\nallow:\n  - a\nheaders:\n  X-Env: prod\n";
    let schema = ServerSpec::schema();
    let tree = Yaml.parse_tree(text).expect("the buffer parses");
    let uri = doc_uri();
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(true, true),
        &uri,
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let kind_of = |name: &str| symbols.iter().find(|s| s.name == name).map(|s| s.kind);
    assert_eq!(kind_of("port"), Some(SymbolKind::FIELD));
    assert_eq!(kind_of("allow"), Some(SymbolKind::ARRAY));
    assert_eq!(kind_of("headers"), Some(SymbolKind::OBJECT));
}

#[test]
fn toml_repeated_table_symbols_do_not_overlap() {
    // Arrange
    let text = "[[upstream]]\nname = \"api\"\nhost = \"h\"\nport = 1\n\n[[upstream]]\nname = \"web\"\nhost = \"h2\"\nport = 2\n";
    let schema = GatewaySpec::schema();
    let tree = Toml.parse_tree(text).expect("the buffer parses");
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(Toml.block_span_covers_body(), true),
        &doc_uri(),
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let upstreams: Vec<_> = symbols.iter().filter(|s| s.name == "upstream").collect();
    assert_eq!(upstreams.len(), 2);
    assert!(
        upstreams[0].range.end <= upstreams[1].range.start,
        "sibling instance ranges stay disjoint: {:?} then {:?}",
        upstreams[0].range,
        upstreams[1].range
    );
    for symbol in &upstreams {
        assert!(
            symbol.range.start <= symbol.selection_range.start
                && symbol.selection_range.end <= symbol.range.end,
            "the selection sits inside its own instance"
        );
        let children = symbol.children.as_ref().expect("children");
        for child in children {
            assert!(
                symbol.range.start <= child.range.start && child.range.end <= symbol.range.end,
                "each child stays inside its own instance"
            );
        }
    }
}

#[test]
fn an_empty_label_is_no_navigation_site() {
    // Arrange
    // The pipeline rejects an empty label, so navigation answers nothing from
    // it, matching definition's answer on the empty reference.
    let text = "upstream:\n  - name: \"\"\n    host: h\n    port: 1\nroutes:\n  - prefix: /a\n    upstream: \"\"\n";
    let offset = text.find("name: \"\"").unwrap() + "name: \"".len();
    let schema = GatewaySpec::schema();

    // Act
    let found = references_at(&Yaml, &schema, text, offset, true);

    // Assert
    assert!(
        found.is_empty(),
        "an empty label defines nothing: {found:?}"
    );
}

const GATEWAY_TOML: &str = "[[upstream]]\nname = \"api\"\nhost = \"h\"\nport = 1\n\n[[routes]]\nprefix = \"/a\"\nupstream = \"api\"\n\n[[routes]]\nprefix = \"/b\"\nupstream = \"api\"\n";

const GATEWAY_KDL: &str = "upstream \"api\" {\n  host \"h\"\n  port 1\n}\nroutes {\n  prefix \"/a\"\n  upstream \"api\"\n}\nroutes {\n  prefix \"/b\"\n  upstream \"api\"\n}\n";

const GATEWAY_JSON: &str = "{\n  \"upstream\": [{ \"name\": \"api\", \"host\": \"h\", \"port\": 1 }],\n  \"routes\": [\n    { \"prefix\": \"/a\", \"upstream\": \"api\" },\n    { \"prefix\": \"/b\", \"upstream\": \"api\" }\n  ]\n}\n";

const GATEWAY_HCL_TWO_UPSTREAMS: &str = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nupstream \"web\" {\n  host = \"h2\"\n  port = 2\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\nroutes {\n  prefix = \"/b\"\n  upstream = \"api\"\n}\nroutes {\n  prefix = \"/c\"\n  upstream = \"web\"\n}\n";

#[test]
fn references_from_a_later_native_label_pick_that_label_not_the_first() {
    // Arrange
    // Two upstreams declare `api` and `web`. The cursor sits in the second
    // label, `web`, named by one route. The span-contains check must land on
    // `web`, not fall back to the first label `api`, which two routes name.
    let text = GATEWAY_HCL_TWO_UPSTREAMS;
    let offset = text.find("\"web\"").unwrap() + 1;
    let schema = GatewaySpec::schema();
    let index = LineIndex::new(text);

    // Act
    let found = references_at(&Hcl, &schema, text, offset, false);

    // Assert
    assert_eq!(found.len(), 1, "only the route naming web: {found:?}");
    assert!(
        covered(&found[0], text, &index).contains("web"),
        "the single hit is the web reference"
    );
}

#[test]
fn kdl_native_label_answers_references_and_no_definition() {
    // Arrange
    let text = GATEWAY_KDL;
    let offset = text.find("\"api\"").unwrap() + 1;
    let schema = GatewaySpec::schema();

    // Act
    let found = references_at(&Kdl, &schema, text, offset, false);
    let definition = definition_at(&Kdl, &schema, text, offset);

    // Assert
    assert_eq!(found.len(), 2, "both routes name the label: {found:?}");
    assert!(definition.is_none(), "a label is its own definition");
}

#[test]
fn toml_designated_label_field_answers_references_and_no_definition() {
    // Arrange
    let text = GATEWAY_TOML;
    let offset = text.find("name = \"api\"").unwrap() + "name = \"a".len();
    let schema = GatewaySpec::schema();

    // Act
    let found = references_at(&Toml, &schema, text, offset, false);
    let definition = definition_at(&Toml, &schema, text, offset);

    // Assert
    assert_eq!(found.len(), 2, "both routes name the label: {found:?}");
    assert!(definition.is_none(), "the label field is the definition");
}

#[test]
fn json_designated_label_field_answers_references() {
    // Arrange
    let text = GATEWAY_JSON;
    let offset = text.find("\"name\": \"api\"").unwrap() + "\"name\": \"a".len();
    let schema = GatewaySpec::schema();

    // Act
    let found = references_at(&Json, &schema, text, offset, true);

    // Assert
    assert_eq!(found.len(), 3, "the declaration and both routes: {found:?}");
}

#[test]
fn toml_and_json_reference_values_answer_a_definition() {
    // Arrange
    let schema = GatewaySpec::schema();
    let toml_offset = GATEWAY_TOML.find("upstream = \"api\"").unwrap() + "upstream = \"a".len();
    let json_offset =
        GATEWAY_JSON.find("\"upstream\": \"api\"").unwrap() + "\"upstream\": \"a".len();

    // Act
    let toml_definition = definition_at(&Toml, &schema, GATEWAY_TOML, toml_offset);
    let json_definition = definition_at(&Json, &schema, GATEWAY_JSON, json_offset);

    // Assert
    assert!(toml_definition.is_some(), "the TOML reference resolves");
    assert!(json_definition.is_some(), "the JSON reference resolves");
}

#[test]
fn the_non_site_positions_answer_empty() {
    // Arrange
    let text = GATEWAY_YAML;
    let schema = GatewaySpec::schema();
    let name_offset = text.find("name: api").unwrap() + 1;
    let plain_offset = text.find("host: h").unwrap() + "host: ".len();
    let no_parse = "routes:\n  - upstream: \"api\"\nbad: [\n";

    // Act
    let name_definition = definition_at(&Yaml, &schema, text, name_offset);
    let plain_definition = definition_at(&Yaml, &schema, text, plain_offset);
    let no_parse_references = references_at(
        &Yaml,
        &schema,
        no_parse,
        no_parse.find("api").unwrap(),
        true,
    );

    // Assert
    assert!(name_definition.is_none(), "a field name defines nothing");
    assert!(
        plain_definition.is_none(),
        "an ordinary value defines nothing"
    );
    assert!(no_parse_references.is_empty(), "navigation needs a parse");
}

#[test]
fn hcl_native_label_details_the_container_symbol() {
    // Arrange
    let text = GATEWAY_HCL;
    let schema = GatewaySpec::schema();
    let tree = Hcl.parse_tree(text).expect("the buffer parses");
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(true, true),
        &doc_uri(),
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let upstream = symbols
        .iter()
        .find(|s| s.name == "upstream")
        .expect("upstream");
    assert_eq!(upstream.detail.as_deref(), Some("api"));
}

#[test]
fn a_single_nested_block_is_a_container_symbol() {
    // Arrange
    let text = "hostname: h\nport: 1\nlimits:\n  mode: \"enforce\"\n";
    let schema = ServerSpec::schema();
    let tree = Yaml.parse_tree(text).expect("the buffer parses");
    let index = LineIndex::new(text);

    // Act
    let response = document_symbols(
        &schema,
        &tree,
        SymbolShape::new(true, true),
        &doc_uri(),
        text,
        &index,
        ENCODING,
    );

    // Assert
    let DocumentSymbolResponse::Nested(symbols) = response else {
        panic!("the hierarchical form");
    };
    let limits = symbols.iter().find(|s| s.name == "limits").expect("limits");
    assert_eq!(limits.kind, SymbolKind::STRUCT);
    let children = limits.children.as_ref().expect("children");
    assert!(children.iter().any(|c| c.name == "mode"));
}
