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
fn empty_document_resolves_to_the_root_body() {
    // Arrange
    let frontend = Hcl;

    // Act
    let context = resolve(&frontend, "", 0);

    // Assert
    assert_eq!(context.path, Vec::<String>::new());
    assert_eq!(context.kind, PositionKind::Body);
    assert_eq!(context.token, None);
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
    let (start, end) = context.token.expect("the scanned identifier");
    assert_eq!(&text[start..end], "work");
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
