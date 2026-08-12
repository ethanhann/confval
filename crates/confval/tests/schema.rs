//! The schema IR, `#[derive(Spec)]`'s type-level walk. It pins the whole shape
//! of a representative fixture by field access, then covers the shapes and leaf
//! types the fixture does not hold with named auxiliary specs.
//!
//! The node types are `#[non_exhaustive]`, so these tests read fields and assert
//! properties rather than constructing an expected `Schema` with a struct
//! literal.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "derive")]

use confval::prelude::*;
use confval::schema::{Constraint, ScalarType, Schema, SchemaType};
use std::collections::BTreeMap;
use std::path::PathBuf;

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);
range_constraint!(RATIO, f64, min: 0.0, max: 1.0);
range_constraint!(HUGE, i64, min: 1, max: i64::MAX);

keyword_enum!(LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

/// The `common` fixture, mirrored here with the two recording attributes so the
/// IR is pinned against a representative Spec rather than only its first
/// consumer.
#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    #[confval(range = PORT)]
    port: Located<i64>,
    #[confval(default = 4, range = WORKERS)]
    workers: Located<i64>,
    #[confval(default = false)]
    tls: Located<bool>,
    #[confval(default)]
    allow: Vec<Located<String>>,
    #[confval(map, default)]
    headers: BTreeMap<String, Located<String>>,
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    #[confval(default = 16, range = MAX_BODY_MB)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string(), keywords = LimitMode)]
    mode: Located<String>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// The shapes and leaf types the fixture does not hold: an optional leaf, an
/// optional-wrapped string list, an `f64` leaf with a range, a `PathBuf` leaf,
/// and a field carrying its own `///` doc.
#[derive(confval::Spec)]
struct CoverageSpec {
    maybe_name: Option<Located<String>>,
    maybe_tags: Option<Located<Vec<Located<String>>>>,
    #[confval(range = RATIO)]
    ratio: Located<f64>,
    log_path: Located<PathBuf>,
    /// The document title shown in the header.
    title: Located<String>,
    #[confval(range = HUGE)]
    big: Located<i64>,
}

impl Validate for CoverageSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// A required nested block and a repeated one, the two `Block` shapes the
/// fixture does not hold.
#[derive(confval::Spec)]
struct BlocksSpec {
    #[confval(nested)]
    child: Located<LimitsSpec>,
    #[confval(nested)]
    many: Vec<Located<LimitsSpec>>,
}

impl Validate for BlocksSpec {
    fn validate(&self, _report: &mut Report) {}
}

// A spec carrying its own `///` doc, so `Schema::doc` has a value to assert.
/// The server's top-level configuration.
#[derive(confval::Spec)]
struct DocumentedSpec {
    hostname: Located<String>,
}

impl Validate for DocumentedSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// Finds a field by its config-key name.
fn field<'a>(schema: &'a Schema, name: &str) -> &'a confval::schema::SchemaField {
    schema
        .fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("field `{name}` is present"))
}

/// The scalar leaf of a field, or a panic when the field is not a scalar.
fn leaf(schema: &Schema, name: &str) -> ScalarType {
    match &field(schema, name).ty {
        SchemaType::Scalar { leaf, .. } => *leaf,
        other => panic!("field `{name}` should be a scalar, was {other:?}"),
    }
}

/// The constraint of a scalar field, or a panic when the field is not a scalar.
fn constraint<'a>(schema: &'a Schema, name: &str) -> &'a Option<Constraint> {
    match &field(schema, name).ty {
        SchemaType::Scalar { constraint, .. } => constraint,
        other => panic!("field `{name}` should be a scalar, was {other:?}"),
    }
}

#[test]
fn the_schema_lists_every_field_in_declaration_order() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let names: Vec<&str> = schema.fields.iter().map(|f| f.name.as_str()).collect();

    // Assert
    assert_eq!(
        names,
        vec![
            "hostname", "port", "workers", "tls", "allow", "headers", "limits"
        ]
    );
}

#[test]
fn a_leaf_reads_its_scalar_type_from_the_rust_type() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let hostname = leaf(&schema, "hostname");
    let port = leaf(&schema, "port");
    let tls = leaf(&schema, "tls");

    // Assert
    assert_eq!(hostname, ScalarType::String);
    assert_eq!(port, ScalarType::Int);
    assert_eq!(tls, ScalarType::Bool);
}

#[test]
fn a_required_leaf_is_required_and_carries_no_default() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let hostname = field(&schema, "hostname");
    let port = field(&schema, "port");

    // Assert
    assert!(hostname.required);
    assert!(!hostname.has_default);
    assert!(port.required);
    assert!(!port.has_default);
}

#[test]
fn a_defaulted_field_is_not_required_whatever_its_shape() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let workers = field(&schema, "workers");
    let allow = field(&schema, "allow");
    let headers = field(&schema, "headers");

    // Assert
    // workers, allow, and headers all sit in the structurally-required column,
    // yet each carries a default, so required folds to false.
    assert!(!workers.required);
    assert!(workers.has_default);
    assert!(!allow.required);
    assert!(allow.has_default);
    assert!(!headers.required);
    assert!(headers.has_default);
}

#[test]
fn an_optional_nested_block_is_not_required_and_has_no_default() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let limits = field(&schema, "limits");

    // Assert
    assert!(!limits.required);
    assert!(!limits.has_default);
    assert!(matches!(
        limits.ty,
        SchemaType::Block {
            repeated: false,
            ..
        }
    ));
}

#[test]
fn a_bare_string_list_is_a_string_list() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let allow = &field(&schema, "allow").ty;

    // Assert
    assert!(matches!(allow, SchemaType::StringList));
}

#[test]
fn a_map_field_is_a_string_map() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let headers = &field(&schema, "headers").ty;

    // Assert
    assert!(matches!(headers, SchemaType::StringMap));
}

#[test]
fn a_block_recurses_into_the_child_schema() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let SchemaType::Block { schema: child, .. } = &field(&schema, "limits").ty else {
        panic!("limits should be a block");
    };

    // Assert
    let child_names: Vec<&str> = child.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, vec!["max_body_mb", "mode"]);
    assert_eq!(leaf(child, "max_body_mb"), ScalarType::Int);
    assert_eq!(leaf(child, "mode"), ScalarType::String);
}

#[test]
fn a_keyword_field_carries_its_keyword_set() {
    // Arrange
    let schema = ServerSpec::schema();
    let SchemaType::Block { schema: limits, .. } = &field(&schema, "limits").ty else {
        panic!("limits should be a block");
    };

    // Act
    let Some(Constraint::Keywords(keywords)) = constraint(limits, "mode") else {
        panic!("mode should carry a keyword set");
    };

    // Assert
    assert_eq!(keywords.to_vec(), vec!["enforce", "log", "off"]);
    assert_eq!(keywords.to_vec(), LimitMode::KEYWORDS.to_vec());
}

#[test]
fn a_range_field_renders_its_bounds_to_text() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let Some(Constraint::Range {
        min,
        max,
        units,
        help,
    }) = constraint(&schema, "port")
    else {
        panic!("port should carry a range");
    };

    // Assert
    assert_eq!(min.as_str(), "1");
    assert_eq!(max.as_str(), "65535");
    assert!(units.is_none());
    assert!(help.is_none());
}

#[test]
fn a_workers_range_reads_from_its_own_constraint() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let Some(Constraint::Range { min, max, .. }) = constraint(&schema, "workers") else {
        panic!("workers should carry a range");
    };

    // Assert
    assert_eq!(min.as_str(), "1");
    assert_eq!(max.as_str(), "512");
}

#[test]
fn a_range_renders_an_i64_extreme_as_its_literal_text() {
    // Arrange
    let schema = CoverageSpec::schema();

    // Act
    let Some(Constraint::Range { max, .. }) = constraint(&schema, "big") else {
        panic!("big should carry a range");
    };

    // Assert
    assert_eq!(max.as_str(), "9223372036854775807");
}

#[test]
fn an_optional_leaf_is_a_scalar_and_not_required() {
    // Arrange
    let schema = CoverageSpec::schema();

    // Act
    let maybe_name = field(&schema, "maybe_name");

    // Assert
    assert_eq!(leaf(&schema, "maybe_name"), ScalarType::String);
    assert!(!maybe_name.required);
    assert!(!maybe_name.has_default);
}

#[test]
fn an_optional_wrapped_string_list_is_a_string_list_and_not_required() {
    // Arrange
    let schema = CoverageSpec::schema();

    // Act
    let maybe_tags = field(&schema, "maybe_tags");

    // Assert
    assert!(matches!(maybe_tags.ty, SchemaType::StringList));
    assert!(!maybe_tags.required);
    assert!(!maybe_tags.has_default);
}

#[test]
fn a_float_leaf_reads_from_the_rust_type_and_carries_a_range() {
    // Arrange
    let schema = CoverageSpec::schema();

    // Act
    let ratio_leaf = leaf(&schema, "ratio");
    let Some(Constraint::Range { min, max, .. }) = constraint(&schema, "ratio") else {
        panic!("ratio should carry a range");
    };

    // Assert
    assert_eq!(ratio_leaf, ScalarType::Float);
    assert_eq!(min.as_str(), "0");
    assert_eq!(max.as_str(), "1");
}

#[test]
fn a_path_leaf_reads_as_path() {
    // Arrange
    let schema = CoverageSpec::schema();

    // Act
    let log_path = leaf(&schema, "log_path");

    // Assert
    assert_eq!(log_path, ScalarType::Path);
}

#[test]
fn a_field_doc_comment_reaches_the_schema_field() {
    // Arrange
    let schema = CoverageSpec::schema();

    // Act
    let title = field(&schema, "title");

    // Assert
    assert_eq!(
        title.doc.as_deref(),
        Some("The document title shown in the header.")
    );
}

#[test]
fn a_field_without_a_doc_comment_carries_none() {
    // Arrange
    let schema = CoverageSpec::schema();

    // Act
    let ratio = field(&schema, "ratio");

    // Assert
    assert!(ratio.doc.is_none());
}

#[test]
fn a_required_nested_block_is_required() {
    // Arrange
    let schema = BlocksSpec::schema();

    // Act
    let child = field(&schema, "child");

    // Assert
    assert!(child.required);
    assert!(!child.has_default);
    assert!(matches!(
        child.ty,
        SchemaType::Block {
            repeated: false,
            ..
        }
    ));
}

#[test]
fn a_nested_list_is_a_repeated_block_and_not_required() {
    // Arrange
    let schema = BlocksSpec::schema();

    // Act
    let many = field(&schema, "many");

    // Assert
    assert!(!many.required);
    assert!(!many.has_default);
    assert!(matches!(many.ty, SchemaType::Block { repeated: true, .. }));
}

#[test]
fn the_struct_doc_comment_reaches_the_schema_doc() {
    // Arrange
    let schema = DocumentedSpec::schema();

    // Act
    let doc = schema.doc.as_deref();

    // Assert
    assert_eq!(doc, Some("The server's top-level configuration."));
}
