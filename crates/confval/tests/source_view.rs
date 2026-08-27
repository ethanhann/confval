//! Tests for the source view: `to_source_fields` emits only the fields the
//! source set, keyed on the attached span, and the three representations of one
//! loaded spec answer their three questions.
//!
//! Each per-shape test drives one row of the spec's semantics table, asserting
//! presence when the source set the field and absence when a default filled it.
//! Parsing supplies the real spans the walk reads, so the fixtures load from
//! TOML rather than build detached instances, except where a hand-built
//! detached value is the case under test.

use confval::format::hcl::emit_hcl;
use confval::format::kdl::emit_kdl;
use confval::format::toml::{emit_toml, parse_toml};
use confval::prelude::*;

confval::keyword_enum!(Mode, {
    Enforce => "enforce",
    Log     => "log",
});

#[derive(confval::Spec, Debug)]
#[confval(derive_default)]
struct Inner {
    #[confval(default = 16)]
    size: Located<i64>,
}

impl Validate for Inner {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, Debug)]
struct Server {
    hostname: Located<String>,
    #[confval(default = 4)]
    workers: Located<i64>,
    port: Option<Located<i64>>,
    #[confval(nested)]
    required_block: Located<Inner>,
    #[confval(nested, default)]
    defaulted_block: Located<Inner>,
    #[confval(nested)]
    optional_block: Option<Located<Inner>>,
    #[confval(nested, default)]
    marked_block: Option<Located<Inner>>,
    #[confval(nested)]
    services: Vec<Located<Inner>>,
    allow: Option<Located<Vec<Located<String>>>>,
    #[confval(default)]
    tags: Vec<Located<String>>,
}

impl Validate for Server {
    fn validate(&self, _report: &mut Report) {}
}

fn parse(text: &str) -> Server {
    let mut sources = SourceMap::new();
    let id = sources.add("server.toml", text.to_string());
    let mut report = Report::new();
    let spec = parse_toml::<Server>(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("fixture should parse: {:?}", report.issues()));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

fn source_names(spec: &Server) -> Vec<String> {
    spec.to_source_fields()
        .iter()
        .map(|field| field.name.clone())
        .collect()
}

#[test]
fn source_view_omits_defaults_and_absent_fields() {
    // Arrange
    // Only the required leaf and the required block are set. Every optional and
    // defaulted field is left out, so a filled default keeps the detached
    // sentinel and the source view drops it.
    let spec = parse("hostname = \"h\"\n\n[required_block]\n");

    // Act
    let names = source_names(&spec);

    // Assert
    assert_eq!(names, vec!["hostname", "required_block"]);
}

#[test]
fn source_view_includes_every_field_the_source_set() {
    // Arrange
    let spec = parse(
        r#"hostname = "h"
workers = 8
port = 9090
allow = ["a", "b"]
tags = ["x"]

[required_block]
size = 1

[defaulted_block]
size = 2

[optional_block]
size = 3

[marked_block]
size = 4

[[services]]
size = 5
"#,
    );

    // Act
    let names = source_names(&spec);

    // Assert
    assert_eq!(
        names,
        vec![
            "hostname",
            "workers",
            "port",
            "required_block",
            "defaulted_block",
            "optional_block",
            "marked_block",
            "services",
            "allow",
            "tags",
        ]
    );
}

#[test]
fn source_view_emits_a_source_written_block_with_defaulted_contents_as_empty() {
    // Arrange
    // The block is written, so its span is attached, but its one inner field is
    // defaulted, so the recursive filter drops the field and the block is empty.
    let spec = parse("hostname = \"h\"\n\n[required_block]\n");

    // Act
    let toml = emit_toml(&spec.to_source_fields()).expect("emit toml");

    // Assert
    assert!(toml.contains("[required_block]"), "got:\n{toml}");
    assert!(!toml.contains("size"), "got:\n{toml}");
}

/// A spec built by hand rather than parsed, so every span is the detached
/// sentinel. `tags` varies, because the bare list is the one shape whose
/// elements keep the only spans it has.
fn detached_server(tags: Vec<Located<String>>) -> Server {
    Server {
        hostname: Located::detached("h".to_string()),
        workers: Located::detached(4),
        port: None,
        required_block: Located::detached(Inner {
            size: Located::detached(16),
        }),
        defaulted_block: Located::detached(Inner {
            size: Located::detached(16),
        }),
        optional_block: None,
        marked_block: None,
        services: vec![],
        allow: None,
        tags,
    }
}

#[test]
fn source_view_filters_the_contents_of_each_repeated_block() {
    // Arrange
    // Two `[[services]]` elements, the first setting its one inner field and
    // the second leaving it defaulted, so the filter has to run per element.
    let spec =
        parse("hostname = \"h\"\n\n[required_block]\n\n[[services]]\nsize = 5\n\n[[services]]\n");

    // Act
    let toml = emit_toml(&spec.to_source_fields()).expect("emit toml");

    // Assert
    assert_eq!(toml.matches("[[services]]").count(), 2, "got:\n{toml}");
    assert_eq!(toml.matches("size").count(), 1, "got:\n{toml}");
    assert!(toml.contains("size = 5"), "got:\n{toml}");
}

#[test]
fn a_hand_built_detached_required_leaf_is_omitted() {
    // Arrange
    // A required leaf cannot be absent from a parsed source, so the detached
    // case is only reachable by hand. The source view treats it as not set.
    let spec = detached_server(vec![]);

    // Act
    let names = source_names(&spec);

    // Assert
    assert!(names.is_empty(), "got: {names:?}");
}

#[test]
fn a_hand_built_detached_bare_list_is_omitted() {
    // Arrange
    // A non-empty bare list whose elements are all detached was never written
    // by a source. Deserializing a spec produces exactly this, because
    // `Located`'s Deserialize attaches the sentinel to every element.
    let spec = detached_server(vec![
        Located::detached("a".to_string()),
        Located::detached("b".to_string()),
    ]);

    // Act
    let names = source_names(&spec);

    // Assert
    assert!(names.is_empty(), "got: {names:?}");
}

#[test]
fn a_wrapped_empty_list_survives_the_source_view() {
    // Arrange
    // The wrapper keeps its own span, so a source-written empty list is
    // distinguishable from an absent one and stays in the view.
    let spec = parse("hostname = \"h\"\nallow = []\n\n[required_block]\n");

    // Act
    let names = source_names(&spec);

    // Assert
    assert!(names.contains(&"allow".to_string()), "got: {names:?}");
}

#[test]
fn a_bare_empty_list_is_omitted_from_the_source_view() {
    // Arrange
    // The bare list holds no wrapper span, so a source-written empty list is
    // indistinguishable from an absent one, and both are dropped.
    let spec = parse("hostname = \"h\"\ntags = []\n\n[required_block]\n");

    // Act
    let names = source_names(&spec);

    // Assert
    assert!(!names.contains(&"tags".to_string()), "got: {names:?}");
}

#[test]
fn source_view_carries_real_value_spans() {
    // Arrange
    // The policy is that each emitted value keeps its source span, while the
    // name span and the container stay detached.
    let spec = parse("hostname = \"h\"\n\n[required_block]\n");

    // Act
    let fields = spec.to_source_fields();
    let hostname = fields.get("hostname").expect("hostname is present");

    // Assert
    assert!(!hostname.span.is_detached(), "field span is attached");
    assert!(hostname.name_span.is_detached(), "name span is detached");
    match &hostname.kind {
        confval::format::FieldKind::Value(value) => {
            assert!(!value.span.is_detached(), "value span is attached")
        }
        other => panic!("expected a value, got {other:?}"),
    }
}

#[derive(confval::Spec, Debug)]
#[confval(derive_default)]
struct LimitsSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Config, serde::Serialize)]
#[confval(lower_from = LimitsSpec)]
struct LimitsConfig {
    #[confval(lower(from = max_body_mb, with = narrow::i64_to_u16))]
    max_body_mb: u16,
    #[confval(lower(from = mode, with = narrow::keyword::<Mode>))]
    mode: Mode,
}

#[test]
fn the_three_views_answer_their_three_questions() {
    // Arrange
    // The source sets only the keyword field, leaving max_body_mb defaulted.
    let mut sources = SourceMap::new();
    let id = sources.add("limits.toml", "mode = \"log\"\n".to_string());
    let mut report = Report::new();
    let spec = parse_toml::<LimitsSpec>(&sources, id, &mut report).expect("parses");

    // Act
    let source = emit_toml(&spec.to_source_fields()).expect("emit source");
    let populated = emit_toml(&spec.to_fields()).expect("emit populated");
    let config = LimitsConfig::lower(&spec, &mut report).expect("lowers");
    let runtime = serde_json::to_string(&config).expect("serialize");

    // Assert
    // The source view shows only what was set.
    assert!(source.contains("mode = \"log\""), "source:\n{source}");
    assert!(!source.contains("max_body_mb"), "source:\n{source}");
    // The populated view fills the default.
    assert!(
        populated.contains("max_body_mb = 16"),
        "populated:\n{populated}"
    );
    assert!(
        populated.contains("mode = \"log\""),
        "populated:\n{populated}"
    );
    // The runtime view shows the lowered values, and the keyword serializes as its
    // keyword rather than the variant name.
    assert_eq!(runtime, r#"{"max_body_mb":16,"mode":"log"}"#);
}

#[test]
fn an_empty_source_view_renders_empty_in_every_format() {
    // Arrange
    // A spec built entirely from defaults has every span detached, so its
    // source view is an empty level, which each emitter renders as empty text.
    let spec = LimitsSpec::default();
    let fields = spec.to_source_fields();

    // Act
    let toml = emit_toml(&fields).expect("emit toml");
    let hcl = emit_hcl(&fields).expect("emit hcl");
    let kdl = emit_kdl(&fields).expect("emit kdl");

    // Assert
    assert!(toml.trim().is_empty(), "toml:\n{toml}");
    assert!(hcl.trim().is_empty(), "hcl:\n{hcl}");
    assert!(kdl.trim().is_empty(), "kdl:\n{kdl}");
}
