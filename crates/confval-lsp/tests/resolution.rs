//! Position resolution against the three block-structured frontends.
//!
//! Each frontend maps a table of offsets against a fixture document to the
//! expected cursor context, including an offset inside a nested block, an offset
//! at an attribute value, an offset in a buffer that does not parse, an empty
//! document, and an offset at end of file.

mod fixture;

use confval_lsp::{CursorContext, Frontend, Hcl, Kdl, PositionKind, Toml};

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
    // A KDL node with no argument parses, and its value span sits on the node
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
