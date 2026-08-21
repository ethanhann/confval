//! The schema IR, `#[derive(Spec)]`'s type-level walk. It pins the whole shape
//! of a representative fixture by field access, then covers the shapes and leaf
//! types the fixture does not hold with named auxiliary specs.
//!
//! The node types are `#[non_exhaustive]`, so these tests read fields and assert
//! properties rather than constructing an expected `Schema` with a struct
//! literal.

#![allow(clippy::unwrap_used, clippy::expect_used)]
use confval::prelude::*;
use confval::schema::{Constraint, ScalarType, Schema, SchemaType};
use std::collections::BTreeMap;
use std::path::PathBuf;

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);
range_constraint!(RATIO, f64, min: 0.0, max: 1.0);
range_constraint!(HUGE, i64, min: 1, max: i64::MAX);
range_constraint!(TIMEOUT, i64, min: 1, max: 300, units: "seconds", help: "Keep this under 5 minutes.");

keyword_enum!(LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

/// The `common` fixture, mirrored here with the two recording attributes so the
/// IR is pinned against a representative Spec rather than only its first
/// consumer. The recording attributes drive the checks, so the `Validate`
/// bodies carry no line for them.
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
    #[confval(default, keywords = LimitMode)]
    modes: Vec<Located<String>>,
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
/// optional leaf carrying a constraint, an optional-wrapped string list, an
/// `f64` leaf with a range, a `PathBuf` leaf, a range with units and help, an
/// `i64::MAX` bound, and a field carrying its own `///` doc.
#[derive(confval::Spec)]
struct CoverageSpec {
    maybe_name: Option<Located<String>>,
    #[confval(keywords = LimitMode)]
    maybe_mode: Option<Located<String>>,
    maybe_tags: Option<Located<Vec<Located<String>>>>,
    #[confval(range = RATIO)]
    ratio: Located<f64>,
    log_path: Located<PathBuf>,
    /// The document title shown in the header.
    title: Located<String>,
    #[confval(range = HUGE)]
    big: Located<i64>,
    #[confval(range = TIMEOUT)]
    timeout: Located<i64>,
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

/// A block whose child carries a `///` while the embedding field does not, so
/// the doc separation has a value to assert on both sides.
#[derive(confval::Spec)]
struct BlockParent {
    #[confval(nested)]
    child: Option<Located<DocumentedChild>>,
}

impl Validate for BlockParent {
    fn validate(&self, _report: &mut Report) {}
}

/// The child block's own documentation.
#[derive(confval::Spec)]
struct DocumentedChild {
    value: Located<i64>,
}

impl Validate for DocumentedChild {
    fn validate(&self, _report: &mut Report) {}
}

// A spec carrying its own `///` doc, so `Schema::doc` has a value to assert.
// These `//` lines stay plain, so only the `///` line below reaches the
// struct's harvested doc.
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
    // Act
    let schema = ServerSpec::schema();

    // Assert
    let names: Vec<&str> = schema.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "hostname", "port", "workers", "tls", "allow", "modes", "headers", "limits"
        ]
    );
}

#[test]
fn a_leaf_reads_its_scalar_type_from_the_rust_type() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    assert_eq!(leaf(&schema, "hostname"), ScalarType::String);
    assert_eq!(leaf(&schema, "port"), ScalarType::Int);
    assert_eq!(leaf(&schema, "tls"), ScalarType::Bool);
}

#[test]
fn a_required_leaf_is_required_and_carries_no_default() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    let hostname = field(&schema, "hostname");
    let port = field(&schema, "port");
    assert!(hostname.required);
    assert!(!hostname.has_default);
    assert!(port.required);
    assert!(!port.has_default);
}

#[test]
fn a_defaulted_field_is_not_required_whatever_its_shape() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    // workers, allow, and headers all sit in the structurally-required column,
    // yet each carries a default, so required folds to false.
    let workers = field(&schema, "workers");
    let allow = field(&schema, "allow");
    let headers = field(&schema, "headers");
    assert!(!workers.required);
    assert!(workers.has_default);
    assert!(!allow.required);
    assert!(allow.has_default);
    assert!(!headers.required);
    assert!(headers.has_default);
}

#[test]
fn an_optional_nested_block_is_not_required_and_has_no_default() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    let limits = field(&schema, "limits");
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
fn a_string_list_records_the_keyword_set_of_its_elements() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    assert_eq!(
        field(&schema, "modes").ty,
        SchemaType::string_list(Some(Constraint::Keywords(&LimitMode::KEYWORDS)))
    );
}

#[test]
fn a_string_list_with_no_attribute_records_no_constraint() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    assert_eq!(field(&schema, "allow").ty, SchemaType::string_list(None));
}

#[test]
fn a_map_field_is_a_string_map() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    assert!(matches!(
        field(&schema, "headers").ty,
        SchemaType::StringMap
    ));
}

#[test]
fn a_block_recurses_into_the_child_schema() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    let SchemaType::Block { schema: child, .. } = &field(&schema, "limits").ty else {
        panic!("limits should be a block");
    };
    let child_names: Vec<&str> = child.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, vec!["max_body_mb", "mode"]);
    assert_eq!(leaf(child, "max_body_mb"), ScalarType::Int);
    assert_eq!(leaf(child, "mode"), ScalarType::String);
}

#[test]
fn a_keyword_field_carries_its_keyword_set() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    let SchemaType::Block { schema: limits, .. } = &field(&schema, "limits").ty else {
        panic!("limits should be a block");
    };
    let Some(Constraint::Keywords(keywords)) = constraint(limits, "mode") else {
        panic!("mode should carry a keyword set");
    };
    assert_eq!(keywords.to_vec(), vec!["enforce", "log", "off"]);
    assert_eq!(keywords.to_vec(), LimitMode::KEYWORDS.to_vec());
}

#[test]
fn a_range_field_renders_its_bounds_to_text() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    let Some(Constraint::Range {
        min,
        max,
        units,
        help,
    }) = constraint(&schema, "port")
    else {
        panic!("port should carry a range");
    };
    assert_eq!(min.as_str(), "1");
    assert_eq!(max.as_str(), "65535");
    assert!(units.is_none());
    assert!(help.is_none());
}

#[test]
fn a_workers_range_reads_from_its_own_constraint() {
    // Act
    let schema = ServerSpec::schema();

    // Assert
    let Some(Constraint::Range { min, max, .. }) = constraint(&schema, "workers") else {
        panic!("workers should carry a range");
    };
    assert_eq!(min.as_str(), "1");
    assert_eq!(max.as_str(), "512");
}

#[test]
fn a_range_renders_an_i64_extreme_as_its_literal_text() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    let Some(Constraint::Range { max, .. }) = constraint(&schema, "big") else {
        panic!("big should carry a range");
    };
    assert_eq!(max.as_str(), "9223372036854775807");
}

#[test]
fn a_range_carries_its_units_and_help() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    let Some(Constraint::Range { units, help, .. }) = constraint(&schema, "timeout") else {
        panic!("timeout should carry a range");
    };
    assert_eq!(*units, Some("seconds"));
    assert_eq!(*help, Some("Keep this under 5 minutes."));
}

#[test]
fn an_optional_leaf_is_a_scalar_and_not_required() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    let maybe_name = field(&schema, "maybe_name");
    assert_eq!(leaf(&schema, "maybe_name"), ScalarType::String);
    assert!(!maybe_name.required);
    assert!(!maybe_name.has_default);
}

#[test]
fn an_optional_leaf_can_carry_a_constraint() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    let maybe_mode = field(&schema, "maybe_mode");
    assert!(!maybe_mode.required);
    assert_eq!(leaf(&schema, "maybe_mode"), ScalarType::String);
    let Some(Constraint::Keywords(keywords)) = constraint(&schema, "maybe_mode") else {
        panic!("maybe_mode should carry a keyword set");
    };
    assert_eq!(keywords.to_vec(), LimitMode::KEYWORDS.to_vec());
}

#[test]
fn an_optional_wrapped_string_list_is_a_string_list_and_not_required() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    let maybe_tags = field(&schema, "maybe_tags");
    assert!(matches!(maybe_tags.ty, SchemaType::StringList { .. }));
    assert!(!maybe_tags.required);
    assert!(!maybe_tags.has_default);
}

#[test]
fn a_float_leaf_reads_from_the_rust_type_and_carries_a_range() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    assert_eq!(leaf(&schema, "ratio"), ScalarType::Float);
    let Some(Constraint::Range { min, max, .. }) = constraint(&schema, "ratio") else {
        panic!("ratio should carry a range");
    };
    // A float bound keeps its float form, so hover on a float field reads
    // float text rather than suggesting integers.
    assert_eq!(min.as_str(), "0.0");
    assert_eq!(max.as_str(), "1.0");
}

#[test]
fn a_path_leaf_reads_as_path() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    assert_eq!(leaf(&schema, "log_path"), ScalarType::Path);
}

#[test]
fn a_field_doc_comment_reaches_the_schema_field() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    let title = field(&schema, "title");
    assert_eq!(
        title.doc.as_deref(),
        Some("The document title shown in the header.")
    );
}

#[test]
fn a_field_without_a_doc_comment_carries_none() {
    // Act
    let schema = CoverageSpec::schema();

    // Assert
    assert!(field(&schema, "ratio").doc.is_none());
}

#[test]
fn a_required_nested_block_is_required() {
    // Act
    let schema = BlocksSpec::schema();

    // Assert
    let child = field(&schema, "child");
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
    // Act
    let schema = BlocksSpec::schema();

    // Assert
    let many = field(&schema, "many");
    assert!(!many.required);
    assert!(!many.has_default);
    assert!(matches!(many.ty, SchemaType::Block { repeated: true, .. }));
}

#[test]
fn a_block_keeps_the_field_doc_and_the_child_doc_separate() {
    // Act
    let schema = BlockParent::schema();

    // Assert
    // The embedding field carries no `///`, so its `SchemaField::doc` is None
    // and never inherits the child struct's doc the template folds in.
    let child_field = field(&schema, "child");
    let SchemaType::Block { schema: child, .. } = &child_field.ty else {
        panic!("child should be a block");
    };
    assert!(child_field.doc.is_none());
    assert_eq!(
        child.doc.as_deref(),
        Some("The child block's own documentation.")
    );
}

#[test]
fn the_struct_doc_comment_reaches_the_schema_doc() {
    // Act
    let schema = DocumentedSpec::schema();

    // Assert
    assert_eq!(
        schema.doc.as_deref(),
        Some("The server's top-level configuration.")
    );
}

/// A spec whose defaults cover every scalar leaf, for the rendered-default
/// carry.
#[derive(confval::Spec)]
struct DefaultedSpec {
    /// An expression default on an integer leaf.
    #[confval(default = 4)]
    workers: Located<i64>,
    /// A whole-number float default, which keeps its `.0`.
    #[confval(default = 4.0)]
    scale: Located<f64>,
    /// A boolean default.
    #[confval(default = true)]
    tls: Located<bool>,
    /// A string default.
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
    /// A path default, rendered through its lossy string form.
    #[confval(default = std::path::PathBuf::from("/etc/app.conf"))]
    config: Located<PathBuf>,
    /// A bare default, which renders the leaf type's own default.
    #[confval(default)]
    retries: Located<i64>,
    /// A bare default on a list, which carries no text.
    #[confval(default)]
    allow: Vec<Located<String>>,
    /// A bare default on a map, which carries no text.
    #[confval(map, default)]
    headers: BTreeMap<String, Located<String>>,
    /// A defaulted nested block, which carries no text.
    #[confval(nested, default)]
    limits: Option<Located<DefaultedChild>>,
    /// No default, which carries no text.
    port: Located<i64>,
}

/// The nested child of the defaulted fixture.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct DefaultedChild {
    /// A defaulted leaf inside the child.
    #[confval(default = 1)]
    depth: Located<i64>,
}

impl Validate for DefaultedChild {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for DefaultedSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn an_expression_default_renders_per_leaf() {
    // Arrange
    let schema = DefaultedSpec::schema();

    // Act
    let texts: Vec<Option<&str>> = ["workers", "scale", "tls", "mode", "config"]
        .iter()
        .map(|name| field(&schema, name).default_text.as_deref())
        .collect();

    // Assert
    assert_eq!(
        texts,
        vec![
            Some("4"),
            Some("4.0"),
            Some("true"),
            Some("enforce"),
            Some("/etc/app.conf"),
        ]
    );
}

#[test]
fn a_bare_default_renders_the_leaf_types_own_default() {
    // Arrange
    let schema = DefaultedSpec::schema();

    // Act
    let text = field(&schema, "retries").default_text.as_deref();

    // Assert
    assert_eq!(text, Some("0"));
}

#[test]
fn a_non_scalar_or_absent_default_carries_no_text() {
    // Arrange
    let schema = DefaultedSpec::schema();

    // Act
    let list = field(&schema, "allow").default_text.as_deref();
    let map = field(&schema, "headers").default_text.as_deref();
    let block = field(&schema, "limits").default_text.as_deref();
    let bare = field(&schema, "port").default_text.as_deref();

    // Assert
    assert_eq!(list, None, "a defaulted list has no single value to render");
    assert_eq!(map, None, "a defaulted map has no single value to render");
    assert_eq!(
        block, None,
        "a defaulted block has no single value to render"
    );
    assert_eq!(bare, None, "an undefaulted field carries nothing");
    assert!(field(&schema, "allow").has_default);
    assert!(field(&schema, "headers").has_default);
    assert!(field(&schema, "limits").has_default);
}

#[test]
fn a_handwritten_field_carries_the_builder_text() {
    // Arrange
    let built = confval::schema::SchemaField::new(
        "workers".to_string(),
        None,
        SchemaType::Scalar {
            leaf: ScalarType::Int,
            constraint: None,
        },
    )
    .required()
    .with_default();

    // Act
    let built = built.with_default_text("4".to_string());

    // Assert
    assert_eq!(built.default_text.as_deref(), Some("4"));
    assert!(built.has_default);
    assert!(!built.required, "a defaulted field is not required");
}

#[test]
fn a_block_and_a_map_record_no_constraint() {
    // Arrange
    let schema = ServerSpec::schema();

    // Act
    let block = field(&schema, "limits").ty.constraint();
    let map = field(&schema, "headers").ty.constraint();

    // Assert
    // A shape that carries no constraint answers `None`, so a reader that
    // renders one asks every field rather than testing the variant first.
    assert!(block.is_none());
    assert!(map.is_none());
}
