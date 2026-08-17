//! Position resolution against the five frontends.
//!
//! Each frontend maps a table of offsets against a fixture document to the
//! expected cursor context, including an offset inside a nested block, an offset
//! at an attribute value, an offset in a buffer that does not parse, an empty
//! document, and an offset at end of file. The JSON and YAML tables add an offset
//! inside a repeated block element and a body under an empty key.

mod fixture;

use confval_lsp::{CursorContext, Frontend, Hcl, Json, Kdl, PositionKind, Toml, Yaml};

/// The byte offset just inside the first occurrence of `needle`.
fn inside(text: &str, needle: &str) -> usize {
    match text.find(needle) {
        Some(offset) => offset + 1,
        None => panic!("needle {needle:?} present in fixture"),
    }
}

/// Resolves an offset against a frontend, parsing the tree first.
fn resolve<F: Frontend>(frontend: &F, text: &str, offset: usize) -> CursorContext {
    let tree = frontend.parse_tree(text);
    frontend.resolve(tree.as_ref(), text, offset)
}

const HCL: &str = "hostname = \"api\"\nport = 8080\nlimits {\n  mode = \"enforce\"\n}\n";
const TOML: &str = "hostname = \"api\"\nport = 8080\n\n[limits]\nmode = \"enforce\"\n";
const KDL: &str = "hostname \"api\"\nport 8080\nlimits {\n  mode \"enforce\"\n}\n";
const JSON: &str = "{\n  \"hostname\": \"api\",\n  \"port\": 8080,\n  \"limits\": { \"mode\": \"enforce\" },\n  \"rules\": [{ \"prefix\": \"/api\" }]\n}\n";
const YAML: &str =
    "hostname: api\nport: 8080\nlimits:\n  mode: enforce\nrules:\n  - prefix: /api\n";

#[test]
fn hcl_offset_table_maps_each_offset() {
    // Arrange
    let frontend = Hcl;

    // Act
    let name = resolve(&frontend, HCL, inside(HCL, "port"));
    let value = resolve(&frontend, HCL, inside(HCL, "8080"));
    let nested_name = resolve(&frontend, HCL, inside(HCL, "mode"));
    let nested_value = resolve(&frontend, HCL, inside(HCL, "enforce"));

    // Assert
    assert_eq!(name.path, Vec::<String>::new());
    assert_eq!(name.kind, PositionKind::Body);
    assert_eq!(value.path, Vec::<String>::new());
    assert_eq!(
        value.kind,
        PositionKind::AttributeValue {
            field: "port".to_string()
        }
    );
    assert_eq!(nested_name.path, vec!["limits".to_string()]);
    assert_eq!(nested_name.kind, PositionKind::Body);
    assert_eq!(nested_value.path, vec!["limits".to_string()]);
    assert_eq!(
        nested_value.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
}

#[test]
fn toml_offset_table_maps_each_offset() {
    // Arrange
    let frontend = Toml;

    // Act
    let value = resolve(&frontend, TOML, inside(TOML, "8080"));
    let nested_name = resolve(&frontend, TOML, inside(TOML, "mode"));
    let nested_value = resolve(&frontend, TOML, inside(TOML, "enforce"));

    // Assert
    assert_eq!(
        value.kind,
        PositionKind::AttributeValue {
            field: "port".to_string()
        }
    );
    assert_eq!(nested_name.path, vec!["limits".to_string()]);
    assert_eq!(nested_name.kind, PositionKind::Body);
    assert_eq!(nested_value.path, vec!["limits".to_string()]);
    assert_eq!(
        nested_value.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
}

#[test]
fn kdl_offset_table_maps_each_offset() {
    // Arrange
    let frontend = Kdl;

    // Act
    let value = resolve(&frontend, KDL, inside(KDL, "8080"));
    let nested_name = resolve(&frontend, KDL, inside(KDL, "mode"));
    let nested_value = resolve(&frontend, KDL, inside(KDL, "enforce"));

    // Assert
    assert_eq!(
        value.kind,
        PositionKind::AttributeValue {
            field: "port".to_string()
        }
    );
    assert_eq!(nested_name.path, vec!["limits".to_string()]);
    assert_eq!(nested_name.kind, PositionKind::Body);
    assert_eq!(nested_value.path, vec!["limits".to_string()]);
    assert_eq!(
        nested_value.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
}

#[test]
fn kdl_value_completion_on_a_valueless_node_inserts_after_the_name() {
    // Arrange
    // A KDL node with no argument parses, and its value span is on the node
    // name. Value completion must insert after the name, so `mode ` becomes
    // `mode "enforce"` and never replaces `mode` with the value.
    let frontend = Kdl;
    let text = "limits {\n  mode \n}\n";
    let offset = text.find("mode ").expect("mode present") + "mode ".len();

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(
        context.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
    let (start, end) = context.token;
    assert_eq!(
        (start, end),
        (offset, offset),
        "the token is a zero-width insert at the cursor, not the node name, got {:?}",
        &text[start..end]
    );
}

#[test]
fn toml_array_of_tables_body_resolves_into_the_element() {
    // Arrange
    let frontend = Toml;
    let text = "[[rules]]\nprefix = \"/a\"\n";
    let offset = text.find("prefix").expect("prefix present") + 1;

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["rules".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn toml_nested_table_body_resolves_into_the_child() {
    // Arrange
    let frontend = Toml;
    let text = "[a]\nx = 1\n[a.b]\ny = 2\n";
    let offset = text.find("y = 2").expect("nested entry present");

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn toml_cursor_after_a_tables_last_entry_resolves_into_the_table() {
    // Arrange
    // A TOML table header spans only the header, so a cursor on a fresh line
    // after the table's last entry must still resolve into the table, where a
    // new key would belong.
    let frontend = Toml;
    let text = "hostname = \"api\"\n[limits]\nmode = \"enforce\"\n";

    // Act
    let context = resolve(&frontend, text, text.len());

    // Assert
    assert_eq!(context.path, vec!["limits".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn toml_cursor_on_the_blank_line_between_tables_resolves_into_the_first() {
    // Arrange
    let frontend = Toml;
    let text = "[limits]\nmode = \"enforce\"\n\n[other]\nx = 1\n";
    let offset = text.find("\n\n").expect("a blank line") + 1;

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["limits".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn empty_document_resolves_to_the_root_body() {
    // Arrange
    let frontend = Hcl;

    // Act
    let context = resolve(&frontend, "", 0);

    // Assert
    assert_eq!(context.path, Vec::<String>::new());
    assert_eq!(context.kind, PositionKind::Body);
    // An empty document has no token, so the replace range is zero-width at the
    // cursor.
    assert_eq!(context.token, (0, 0));
}

#[test]
fn a_buffer_that_does_not_parse_falls_back_to_a_text_scan() {
    // Arrange
    // A half-typed attribute name is a syntax error, so the tree is absent and
    // resolution scans the raw text for the identifier under the cursor.
    let frontend = Hcl;
    let text = "hostname = \"api\"\nwork";

    // Act
    let context = resolve(&frontend, text, text.len());

    // Assert
    assert_eq!(context.path, Vec::<String>::new());
    assert_eq!(context.kind, PositionKind::Body);
    let (start, end) = context.token;
    assert_eq!(&text[start..end], "work");
}

#[test]
fn resolution_recovers_from_text_when_the_buffer_does_not_parse() {
    // Arrange
    // The buffer does not parse, so no tree is available and resolution
    // reconstructs the enclosing block path from the raw text.
    let frontend = Hcl;
    let invalid = "limits {\n  mode : \"enforce\"\n}\n";
    assert!(
        frontend.parse_tree(invalid).is_none(),
        "the buffer does not parse"
    );
    let offset = invalid.find("mode").expect("mode present") + 1;

    // Act
    let context = frontend.resolve(None, invalid, offset);

    // Assert
    assert_eq!(context.path, vec!["limits".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn a_parsed_value_with_a_space_is_replaced_whole() {
    // Arrange
    // The buffer parses, so the tree walk uses the value's exact span. A value
    // with a space must be replaced whole, not split at the space.
    let frontend = Hcl;
    let text = "mode = \"log loud\"\n";
    let offset = text.find("log").expect("value present") + 1;

    // Act
    let context = frontend.resolve(frontend.parse_tree(text).as_ref(), text, offset);

    // Assert
    assert_eq!(
        context.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
    let (start, end) = context.token;
    assert_eq!(&text[start..end], "\"log loud\"");
}

#[test]
fn toml_recovers_at_the_document_edges() {
    // Arrange
    let frontend = Toml;
    let document = "port = 8080\n";

    // Act
    let empty = resolve(&frontend, "", 0);
    let end_of_file = resolve(&frontend, document, document.len());
    let no_parse = resolve(&frontend, "port = ", "port = ".len());

    // Assert
    assert_eq!(empty.kind, PositionKind::Body);
    assert_eq!(end_of_file.path, Vec::<String>::new());
    assert_eq!(end_of_file.kind, PositionKind::Body);
    // `port = ` with an empty value does not parse, and text recovery reads it as
    // a value position rather than a body position.
    assert_eq!(
        no_parse.kind,
        PositionKind::AttributeValue {
            field: "port".to_string()
        }
    );
}

#[test]
fn kdl_recovers_at_the_document_edges() {
    // Arrange
    let frontend = Kdl;
    let document = "port 8080\n";

    // Act
    let empty = resolve(&frontend, "", 0);
    let end_of_file = resolve(&frontend, document, document.len());
    let no_parse = resolve(&frontend, "node \"x", "node \"x".len());

    // Assert
    assert_eq!(empty.kind, PositionKind::Body);
    assert_eq!(end_of_file.path, Vec::<String>::new());
    assert_eq!(end_of_file.kind, PositionKind::Body);
    // The unclosed string does not parse, so recovery reads the node argument.
    assert_eq!(no_parse.path, Vec::<String>::new());
    assert_eq!(
        no_parse.kind,
        PositionKind::AttributeValue {
            field: "node".to_string()
        }
    );
}

#[test]
fn an_offset_at_end_of_file_resolves_to_the_root_body() {
    // Arrange
    let frontend = Hcl;

    // Act
    let context = resolve(&frontend, HCL, HCL.len());

    // Assert
    assert_eq!(context.path, Vec::<String>::new());
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn json_offset_table_maps_each_offset() {
    // Arrange
    let frontend = Json;

    // Act
    let value = resolve(&frontend, JSON, inside(JSON, "8080"));
    let nested_name = resolve(&frontend, JSON, inside(JSON, "mode"));
    let nested_value = resolve(&frontend, JSON, inside(JSON, "enforce"));
    let element_name = resolve(&frontend, JSON, inside(JSON, "prefix"));
    let element_value = resolve(&frontend, JSON, inside(JSON, "/api"));

    // Assert
    assert_eq!(
        value.kind,
        PositionKind::AttributeValue {
            field: "port".to_string()
        }
    );
    assert_eq!(nested_name.path, vec!["limits".to_string()]);
    assert_eq!(nested_name.kind, PositionKind::Body);
    assert_eq!(nested_value.path, vec!["limits".to_string()]);
    assert_eq!(
        nested_value.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
    // An array of objects: a cursor inside a `rules` element resolves into the
    // element, matching the clean walk's array-element entry.
    assert_eq!(element_name.path, vec!["rules".to_string()]);
    assert_eq!(element_name.kind, PositionKind::Body);
    assert_eq!(element_value.path, vec!["rules".to_string()]);
    assert_eq!(
        element_value.kind,
        PositionKind::AttributeValue {
            field: "prefix".to_string()
        }
    );
}

#[test]
fn json_recovers_the_object_path_when_the_buffer_does_not_parse() {
    // Arrange
    // An unclosed object does not parse, so recovery reconstructs the path from
    // the open braces and the property key.
    let frontend = Json;
    let text = "{\n  \"limits\": {\n    \"mode\": \"x\",\n    \n";
    assert!(
        frontend.parse_tree(text).is_none(),
        "the buffer does not parse"
    );
    let offset = text.len() - 1;

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["limits".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn json_recovers_into_an_array_element_when_the_buffer_does_not_parse() {
    // Arrange
    let frontend = Json;
    let text = "{\n  \"rules\": [\n    {\n      \n";
    assert!(
        frontend.parse_tree(text).is_none(),
        "the buffer does not parse"
    );
    let offset = text.len() - 1;

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["rules".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn yaml_offset_table_maps_each_offset() {
    // Arrange
    let frontend = Yaml;

    // Act
    let value = resolve(&frontend, YAML, inside(YAML, "8080"));
    let nested_name = resolve(&frontend, YAML, inside(YAML, "mode"));
    let nested_value = resolve(&frontend, YAML, inside(YAML, "enforce"));
    let element_name = resolve(&frontend, YAML, inside(YAML, "prefix"));
    let element_value = resolve(&frontend, YAML, inside(YAML, "/api"));

    // Assert
    assert_eq!(
        value.kind,
        PositionKind::AttributeValue {
            field: "port".to_string()
        }
    );
    assert_eq!(nested_name.path, vec!["limits".to_string()]);
    assert_eq!(nested_name.kind, PositionKind::Body);
    assert_eq!(nested_value.path, vec!["limits".to_string()]);
    assert_eq!(
        nested_value.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
    assert_eq!(element_name.path, vec!["rules".to_string()]);
    assert_eq!(element_name.kind, PositionKind::Body);
    assert_eq!(element_value.path, vec!["rules".to_string()]);
    assert_eq!(
        element_value.kind,
        PositionKind::AttributeValue {
            field: "prefix".to_string()
        }
    );
}

#[test]
fn yaml_body_under_an_empty_key_resolves_into_it_on_a_clean_parse() {
    // Arrange
    // The `limits:` key awaits its body, which parses as null. A parsing buffer
    // still resolves the indented cursor into limits rather than the root,
    // because YAML resolution reads indentation in both parse states.
    let frontend = Yaml;
    let text = "hostname: api\nlimits:\n  \n";
    assert!(
        frontend.parse_tree(text).is_some(),
        "the buffer parses, with limits null"
    );
    let offset = text.len() - 1;

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["limits".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn json_recovers_a_body_inside_an_inline_array_element() {
    // Arrange
    // The cursor is in a fresh element on the SAME line as the `rules` key, so
    // the value-position scan must not read the `rules` colon as the separator.
    let frontend = Json;
    let text = "{ \"rules\": [{ \"prefix\": \"/a\" }, {  ";
    assert!(
        frontend.parse_tree(text).is_none(),
        "the buffer does not parse"
    );
    let offset = text.len();

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["rules".to_string()]);
    assert_eq!(context.kind, PositionKind::Body);
}

#[test]
fn json_two_levels_deep_collects_both_keys() {
    // Arrange
    let frontend = Json;
    let clean = "{\n  \"a\": {\n    \"b\": {\n      \"c\": 1\n    }\n  }\n}\n";
    let broken = "{\n  \"a\": {\n    \"b\": {\n      \n";

    // Act
    let clean_ctx = resolve(&frontend, clean, clean.find("\"c\"").unwrap() + 1);
    assert!(
        frontend.parse_tree(broken).is_none(),
        "the broken buffer does not parse"
    );
    let broken_ctx = resolve(&frontend, broken, broken.len() - 1);

    // Assert
    assert_eq!(clean_ctx.path, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(clean_ctx.kind, PositionKind::Body);
    assert_eq!(broken_ctx.path, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(broken_ctx.kind, PositionKind::Body);
}

#[test]
fn yaml_value_completion_replaces_the_whole_quoted_value() {
    // Arrange
    // A parsed YAML value with a space is replaced whole, so completing an enum
    // over a quoted value does not stop at the space and corrupt the tail.
    let frontend = Yaml;
    let text = "limits:\n  mode: \"log loud\"\n";
    let offset = text.find("log").unwrap();

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(
        context.kind,
        PositionKind::AttributeValue {
            field: "mode".to_string()
        }
    );
    let (start, end) = context.token;
    assert_eq!(&text[start..end], "\"log loud\"", "the whole quoted value");
}

#[test]
fn yaml_value_token_inside_a_sequence_element_covers_the_parsed_value() {
    // Arrange
    // The reference value in the last sequence element is quoted with a space.
    // Its parsed span must be the replace token, so completion and hover read
    // the whole value rather than stopping at the space.
    let frontend = Yaml;
    let text = "upstream:\n  - name: \"a b\"\n    host: h\n    port: 1\nroutes:\n  - prefix: /x\n    upstream: \"a b\"\n";
    let offset = text.rfind("a b").unwrap();

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(
        context.kind,
        PositionKind::AttributeValue {
            field: "upstream".to_string()
        }
    );
    let (start, end) = context.token;
    assert_eq!(&text[start..end], "\"a b\"", "the whole parsed value");
}

#[test]
fn yaml_offset_at_a_sibling_key_after_a_sequence_reads_the_enclosing_level() {
    // Arrange
    // The cursor is at the first character of the root `port` key, directly
    // after the last sequence element. The element ends strictly before the
    // sibling, so the resolved body is the root, which sets `port`.
    let frontend = Yaml;
    let text = "upstream:\n  - name: a\n    host: h\nport: 8080\n";
    let offset = text.find("\nport").unwrap() + 1;

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, Vec::<String>::new());
    let body = context.resolved_body.as_ref().expect("a parsed body");
    assert!(body.has("port"), "the root level sets port");
}

#[test]
fn yaml_pending_body_under_an_empty_key_carries_an_empty_body() {
    // Arrange
    // `admin:` parses as null, so its body is pending. The resolved body must
    // be empty rather than the parent level, whose fields would leak into the
    // already-set state.
    let frontend = Yaml;
    let text = "port: 8080\nadmin:\n  \n";
    let offset = text.len() - 1;

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(context.path, vec!["admin".to_string()]);
    let body = context.resolved_body.as_ref().expect("a parsed body");
    assert!(!body.has("port"), "a pending body sets nothing");
}

#[test]
fn yaml_empty_value_after_a_colon_keeps_a_zero_width_token_when_the_buffer_parses() {
    // Arrange
    // `upstream:` parses as null, a value outside the model. There is no value
    // text to replace, so the token must stay the zero-width scan result at
    // the cursor rather than the null's span, which covers the colon.
    let frontend = Yaml;
    let text = "rules:\n  - prefix: \"/x\"\n    upstream:\n";
    let offset = text.find("upstream:").unwrap() + "upstream:".len();
    assert!(frontend.parse_tree(text).is_some(), "the buffer parses");

    // Act
    let context = resolve(&frontend, text, offset);

    // Assert
    assert_eq!(
        context.kind,
        PositionKind::AttributeValue {
            field: "upstream".to_string()
        }
    );
    assert_eq!(context.token, (offset, offset), "zero width at the cursor");
}

#[test]
fn a_trailing_comment_in_the_recovery_is_not_a_value_position() {
    // Arrange
    // Each buffer does not parse, and the cursor is inside a trailing
    // comment. The recovery must not classify the comment as the value of the
    // field before it.
    let hcl_hash = "bad = = 1\nport = 1 # note\n";
    let hcl_slash = "bad = = 1\nport = 1 // note\n";
    let toml_hash = "bad = = 1\nport = 1 # note\n";
    let kdl_slash = "bad = = 1\nport 1 // note\n";

    // Act
    let hcl_hash_ctx = Hcl.resolve(None, hcl_hash, hcl_hash.find("note").unwrap() + 1);
    let hcl_slash_ctx = Hcl.resolve(None, hcl_slash, hcl_slash.find("note").unwrap() + 1);
    let toml_ctx = Toml.resolve(None, toml_hash, toml_hash.find("note").unwrap() + 1);
    let kdl_ctx = Kdl.resolve(None, kdl_slash, kdl_slash.find("note").unwrap() + 1);

    // Assert
    assert_eq!(hcl_hash_ctx.kind, PositionKind::Body, "an HCL hash comment");
    assert_eq!(
        hcl_slash_ctx.kind,
        PositionKind::Body,
        "an HCL slash comment"
    );
    assert_eq!(toml_ctx.kind, PositionKind::Body, "a TOML hash comment");
    assert_eq!(kdl_ctx.kind, PositionKind::Body, "a KDL slash comment");
}

#[test]
fn a_comment_marker_inside_a_string_stays_a_value_position() {
    // Arrange
    let text = "bad = = 1\ngreeting = \"a # b\"\n";
    let offset = text.find("# b").unwrap() + 1;

    // Act
    let context = Hcl.resolve(None, text, offset);

    // Assert
    assert_eq!(
        context.kind,
        PositionKind::AttributeValue {
            field: "greeting".to_string()
        },
        "the marker inside the string does not start a comment"
    );
}
