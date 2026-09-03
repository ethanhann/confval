//! Completion against the labeled Gateway and Mesh fixtures, across the
//! formats: per-instance filtering of already-set fields, reference values
//! offering the defined labels, scope-aware label collection, the block-label
//! cursor, and quoting of inserted labels.

mod fixture;
mod support;

use lsp_types::{CompletionTextEdit, Position};

use confval::schema::ToSchema;
use confval_lsp::handlers::{ClientSupport, Cx, completion};
use confval_lsp::{Hcl, Json, Kdl, LineIndex, Toml, Yaml};

use fixture::GatewaySpec;
use support::{ENCODING, MESH_YAML, at_with, gateway_hover, gateway_offered, inserted, labels};

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

    // Act
    let offered = [
        ("hcl", gateway_offered(&Hcl, hcl, hcl_off)),
        ("kdl", gateway_offered(&Kdl, kdl, kdl_off)),
        ("toml", gateway_offered(&Toml, toml, toml_off)),
        ("json", gateway_offered(&Json, json, json_off)),
        ("yaml", gateway_offered(&Yaml, yaml, yaml_off)),
    ];

    // Assert
    for (format, labels) in offered {
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
fn scoped_reference_completion_offers_only_the_own_scope_labels() {
    // Arrange
    // The cursor is in service b's route value. The declaring scope is that
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

#[test]
fn reference_value_completion_omits_a_whitespace_only_label() {
    // Arrange
    // A label of spaces alone names nothing, so the reference value offers the
    // named upstream and not the blank one.
    let hcl = "upstream \"  \" {\n  host = \"h\"\n  port = 1\n}\nupstream \"api\" {\n  host = \"h2\"\n  port = 2\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"\"\n}\n";
    let offset = hcl.rfind("upstream = \"").unwrap() + "upstream = \"".len();

    // Act
    let offered = gateway_offered(&Hcl, hcl, offset);

    // Assert
    assert_eq!(
        offered,
        vec!["api".to_string()],
        "offers the named label alone"
    );
}
