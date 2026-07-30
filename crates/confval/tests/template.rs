//! Tests for template mode: emit with doc comments harvested from spec fields.
//!
//! `to_template` attaches each field's doc comment, and emit renders it as a `#`
//! comment above the field. `to_fields` stays comment-free. The comment source
//! is a `///` doc comment, or a `#[confval(doc = "...")]` override.

use confval::format::hcl::{emit_hcl, parse_hcl};
use confval::format::toml::{emit_toml, parse_toml};
use confval::format::{Fields, FromFields};
use confval::prelude::*;

#[derive(confval::Spec, PartialEq, Debug)]
#[confval(derive_default)]
struct LimitsSpec {
    /// Max body size in MB.
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct ServiceSpec {
    port: Located<i64>,
}

impl Validate for ServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct ServerSpec {
    /// The hostname to bind.
    /// Second line of the comment.
    hostname: Located<String>,
    /// This rustdoc must lose to the attribute.
    #[confval(doc = "The listen port (overridden text).")]
    port: Located<i64>,
    workers: Located<i64>,
    /// Resource limits.
    #[confval(nested, default)]
    limits: Option<Located<LimitsSpec>>,
    /// A service definition.
    #[confval(nested)]
    service: Vec<Located<ServiceSpec>>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn sample() -> ServerSpec {
    ServerSpec {
        hostname: Located::detached("api".to_string()),
        port: Located::detached(8080),
        workers: Located::detached(4),
        limits: None,
        service: vec![
            Located::detached(ServiceSpec {
                port: Located::detached(9001),
            }),
            Located::detached(ServiceSpec {
                port: Located::detached(9002),
            }),
        ],
    }
}

#[test]
fn a_doc_attribute_wins_over_a_rustdoc_on_the_same_field() {
    // Arrange
    // `port` carries both spellings, so this pins the precedence at
    // `options.rs`, where the attribute is consulted before the harvested
    // rustdoc.
    let spec = sample();

    // Act
    let fields = spec.to_template();

    // Assert
    assert_eq!(
        fields.get("port").unwrap().doc.as_deref(),
        Some("The listen port (overridden text).")
    );
    let text = emit_toml(&fields).expect("emit toml");
    assert!(!text.contains("This rustdoc must lose"), "got:\n{text}");
}

#[test]
fn toml_template_carries_the_comments() {
    // Arrange
    let spec = sample();

    // Act
    let text = emit_toml(&spec.to_template()).expect("emit toml");

    // Assert
    // The harvested multi-line comment, both lines.
    assert!(text.contains("# The hostname to bind."), "got:\n{text}");
    assert!(
        text.contains("# Second line of the comment."),
        "got:\n{text}"
    );
    // The attribute override wins over any rustdoc.
    assert!(
        text.contains("# The listen port (overridden text)."),
        "got:\n{text}"
    );
    // A nested field's comment, inside the filled limits table.
    assert!(text.contains("# Max body size in MB."), "got:\n{text}");
    // A repeated block is annotated once, above the first array-of-tables entry.
    assert_eq!(
        text.matches("# A service definition.").count(),
        1,
        "got:\n{text}"
    );
}

#[test]
fn hcl_template_indents_a_nested_comment_and_repeats_a_block_comment() {
    // Arrange
    let spec = sample();

    // Act
    let text = emit_hcl(&spec.to_template()).expect("emit hcl");

    // Assert
    // The nested comment carries the field's one-level indentation.
    assert!(text.contains("  # Max body size in MB."), "got:\n{text}");
    // HCL annotates every repeated block.
    assert_eq!(
        text.matches("# A service definition.").count(),
        2,
        "got:\n{text}"
    );
}

#[test]
fn to_fields_stays_comment_free() {
    // Arrange
    let spec = sample();

    // Act
    let toml = emit_toml(&spec.to_fields()).expect("emit toml");
    let hcl = emit_hcl(&spec.to_fields()).expect("emit hcl");

    // Assert
    assert!(!toml.contains('#'), "toml had a comment:\n{toml}");
    assert!(!hcl.contains('#'), "hcl had a comment:\n{hcl}");
}

fn populated(fields: &Fields) -> ServerSpec {
    let mut report = Report::new();
    let Some(spec) = ServerSpec::from_fields(fields, &mut report) else {
        panic!("fields should parse");
    };
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

fn parse_toml_spec(text: &str) -> ServerSpec {
    let mut sources = SourceMap::new();
    let id = sources.add("t.toml", text.to_string());
    let mut report = Report::new();
    let Some(spec) = parse_toml::<ServerSpec>(&sources, id, &mut report) else {
        panic!("template toml should parse");
    };
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

fn parse_hcl_spec(text: &str) -> ServerSpec {
    let mut sources = SourceMap::new();
    let id = sources.add("t.hcl", text.to_string());
    let mut report = Report::new();
    let Some(spec) = parse_hcl::<ServerSpec>(&sources, id, &mut report) else {
        panic!("template hcl should parse");
    };
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

#[test]
fn a_template_parses_back_to_the_populated_spec() {
    // Arrange
    // Comments are ignored by the parser, so a template reparses to the same
    // spec as the plain populate, which pins that annotation changes only the
    // comments and not the values.
    let expected = populated(&sample().to_fields());

    // Act
    let from_toml = parse_toml_spec(&emit_toml(&sample().to_template()).unwrap());
    let from_hcl = parse_hcl_spec(&emit_hcl(&sample().to_template()).unwrap());

    // Assert
    assert_eq!(from_toml, expected);
    assert_eq!(from_hcl, expected);
}

/// Widget assembly settings.
#[derive(confval::Spec, PartialEq, Debug)]
#[confval(derive_default)]
struct WidgetSpec {
    #[confval(default = 16)]
    max_weight: Located<i64>,
}

impl Validate for WidgetSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct MachineSpec {
    #[confval(nested, default)]
    widget: Located<WidgetSpec>,
    /// The fallback widget.
    #[confval(nested, default)]
    backup: Located<WidgetSpec>,
    /// This rustdoc must lose to the attribute.
    #[confval(nested, default, doc = "The override widget.")]
    gizmo: Located<WidgetSpec>,
}

impl Validate for MachineSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn machine() -> MachineSpec {
    MachineSpec {
        widget: Located::detached(WidgetSpec::default()),
        backup: Located::detached(WidgetSpec::default()),
        gizmo: Located::detached(WidgetSpec::default()),
    }
}

#[test]
fn a_struct_doc_annotates_a_block_whose_field_has_no_doc() {
    // Arrange
    // `widget` carries no doc of its own, so the block's comment falls back to
    // the `///` on `WidgetSpec` itself.
    let spec = machine();

    // Act
    let fields = spec.to_template();

    // Assert
    assert_eq!(
        fields.get("widget").unwrap().doc.as_deref(),
        Some("Widget assembly settings.")
    );
    let toml = emit_toml(&fields).expect("emit toml");
    let hcl = emit_hcl(&fields).expect("emit hcl");
    assert!(
        toml.contains("# Widget assembly settings.\n[widget]"),
        "got:\n{toml}"
    );
    assert!(
        hcl.contains("# Widget assembly settings.\nwidget {"),
        "got:\n{hcl}"
    );
}

#[test]
fn a_field_doc_wins_over_the_struct_doc() {
    // Arrange
    let spec = machine();

    // Act
    let fields = spec.to_template();

    // Assert
    assert_eq!(
        fields.get("backup").unwrap().doc.as_deref(),
        Some("The fallback widget.")
    );
}

#[test]
fn a_doc_attribute_wins_over_the_field_and_struct_docs() {
    // Arrange
    let spec = machine();

    // Act
    let fields = spec.to_template();

    // Assert
    assert_eq!(
        fields.get("gizmo").unwrap().doc.as_deref(),
        Some("The override widget.")
    );
}

#[test]
fn a_struct_doc_never_reaches_to_fields() {
    // Arrange
    let spec = machine();

    // Act
    let fields = spec.to_fields();

    // Assert
    let toml = emit_toml(&fields).expect("emit toml");
    let hcl = emit_hcl(&fields).expect("emit hcl");
    assert!(!toml.contains('#'), "toml had a comment:\n{toml}");
    assert!(!hcl.contains('#'), "hcl had a comment:\n{hcl}");
}

#[derive(confval::Spec, PartialEq, Debug)]
struct FleetSpec {
    #[confval(nested)]
    widget: Vec<Located<WidgetSpec>>,
}

impl Validate for FleetSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_struct_doc_annotates_repeated_blocks_like_a_field_doc_would() {
    // Arrange
    // The fallback follows the field-doc behavior for lists: HCL annotates
    // every repeated block, and TOML annotates the array of tables once.
    let spec = FleetSpec {
        widget: vec![
            Located::detached(WidgetSpec::default()),
            Located::detached(WidgetSpec::default()),
        ],
    };

    // Act
    let fields = spec.to_template();

    // Assert
    let hcl = emit_hcl(&fields).expect("emit hcl");
    let toml = emit_toml(&fields).expect("emit toml");
    assert_eq!(
        hcl.matches("# Widget assembly settings.").count(),
        2,
        "got:\n{hcl}"
    );
    assert_eq!(
        toml.matches("# Widget assembly settings.").count(),
        1,
        "got:\n{toml}"
    );
}

/// A repeated service entry.
#[derive(confval::Spec, PartialEq, Debug)]
struct SvcSpec {
    port: Located<i64>,
}

impl Validate for SvcSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct HiddenShapes {
    port: Located<i64>,
    /// The PID file path.
    pid_file: Option<Located<String>>,
    /// Extra workers beyond the base pool.
    #[confval(default = 4)]
    extra: Option<Located<i64>>,
    /// Allowed CIDRs.
    allow: Option<Located<Vec<Located<String>>>>,
    /// TLS settings.
    #[confval(nested)]
    tls: Option<Located<WidgetSpec>>,
    #[confval(nested)]
    svc: Vec<Located<SvcSpec>>,
}

impl Validate for HiddenShapes {
    fn validate(&self, _report: &mut Report) {}
}

fn hidden() -> HiddenShapes {
    HiddenShapes {
        port: Located::detached(8080),
        pid_file: None,
        extra: None,
        allow: None,
        tls: None,
        svc: vec![],
    }
}

#[test]
fn the_template_walk_emits_commented_fields_for_every_hidden_shape() {
    // Arrange
    let spec = hidden();

    // Act
    let fields = spec.to_template();

    // Assert
    let commented: Vec<&str> = fields
        .iter()
        .filter(|field| field.commented)
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(commented, vec!["pid_file", "extra", "allow", "tls", "svc"]);
    // Each carries its doc, and the empty nested list falls back to the
    // repeated type's own doc.
    let by_name = |name: &str| {
        fields
            .iter()
            .find(|field| field.name == name && field.commented)
            .unwrap()
    };
    assert_eq!(
        by_name("pid_file").doc.as_deref(),
        Some("The PID file path.")
    );
    assert_eq!(
        by_name("svc").doc.as_deref(),
        Some("A repeated service entry.")
    );
    // The name lookup treats them as absent.
    assert!(fields.get("pid_file").is_none());
}

#[test]
fn a_template_renders_every_hidden_shape_as_a_commented_entry() {
    // Arrange
    let spec = hidden();

    // Act
    let toml = emit_toml(&spec.to_template()).expect("emit toml");
    let hcl = emit_hcl(&spec.to_template()).expect("emit hcl");

    // Assert
    for needle in [
        "# The PID file path.\n#pid_file = \"\"",
        "#extra = 4",
        "#allow = []",
        "#[tls]",
        "#[[svc]]",
    ] {
        assert!(toml.contains(needle), "missing {needle:?} in:\n{toml}");
    }
    for needle in [
        "# The PID file path.\n#pid_file = \"\"",
        "#extra = 4",
        "#allow = []",
        "#tls {",
        "#svc {",
    ] {
        assert!(hcl.contains(needle), "missing {needle:?} in:\n{hcl}");
    }
}

#[test]
fn a_commented_template_reparses_to_the_same_spec_as_the_plain_dump() {
    // Arrange
    let spec = hidden();
    let expected = {
        let mut report = Report::new();
        HiddenShapes::from_fields(&spec.to_fields(), &mut report).expect("plain dump parses")
    };

    // Act
    let toml = emit_toml(&spec.to_template()).expect("emit toml");
    let hcl = emit_hcl(&spec.to_template()).expect("emit hcl");

    // Assert
    // The commented entries are invisible to both parsers, so the template
    // means the same configuration as the dump without them.
    let mut sources = SourceMap::new();
    let toml_id = sources.add("t.toml", toml.clone());
    let hcl_id = sources.add("t.hcl", hcl.clone());
    let mut report = Report::new();
    let from_toml: HiddenShapes = parse_toml(&sources, toml_id, &mut report)
        .unwrap_or_else(|| panic!("template toml should parse: {toml}\n{:?}", report.issues()));
    assert!(!report.has_issues(), "toml issues: {:?}", report.issues());
    let from_hcl: HiddenShapes = parse_hcl(&sources, hcl_id, &mut report)
        .unwrap_or_else(|| panic!("template hcl should parse: {hcl}\n{:?}", report.issues()));
    assert!(!report.has_issues(), "hcl issues: {:?}", report.issues());
    assert_eq!(from_toml, expected);
    assert_eq!(from_hcl, expected);
}

#[test]
fn a_template_field_model_parses_the_same_as_its_rendered_text() {
    // Arrange
    let spec = hidden();

    // Act
    let mut report = Report::new();
    let from_model = HiddenShapes::from_fields(&spec.to_template(), &mut report)
        .expect("the walk skips commented fields");

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    let mut plain_report = Report::new();
    let from_plain =
        HiddenShapes::from_fields(&spec.to_fields(), &mut plain_report).expect("plain parses");
    assert_eq!(from_model, from_plain);
}

#[test]
fn the_plain_walk_emits_no_commented_fields() {
    // Arrange
    let spec = hidden();

    // Act
    let fields = spec.to_fields();

    // Assert
    assert!(fields.iter().all(|field| !field.commented));
    assert!(fields.iter().all(|field| field.name != "pid_file"));
}

#[derive(confval::Spec, PartialEq, Debug)]
struct DocShapes {
    /// First line.
    ///
    /// Third line.
    #[doc(hidden)]
    count: Located<i64>,
}

impl Validate for DocShapes {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_blank_doc_line_renders_bare_and_a_doc_list_is_skipped() {
    // Arrange
    let spec = DocShapes {
        count: Located::detached(1),
    };

    // Act
    let text = emit_toml(&spec.to_template()).expect("emit toml");

    // Assert
    // The blank line between the two comment lines renders as a bare `#`, with
    // no trailing space, and the `#[doc(hidden)]` attribute is skipped rather
    // than harvested or errored.
    assert!(
        text.contains("# First line.\n#\n# Third line."),
        "got:\n{text}"
    );
    assert!(
        !text.contains("# \n"),
        "trailing space on blank line:\n{text}"
    );
}
