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
fn toml_template_carries_the_comments() {
    let text = emit_toml(&sample().to_template()).expect("emit toml");
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
    let text = emit_hcl(&sample().to_template()).expect("emit hcl");
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
    let toml = emit_toml(&sample().to_fields()).expect("emit toml");
    let hcl = emit_hcl(&sample().to_fields()).expect("emit hcl");
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
    // Comments are ignored by the parser, so a template reparses to the same
    // spec as the plain populate, which pins that annotation changes only the
    // comments and not the values.
    let expected = populated(&sample().to_fields());
    assert_eq!(
        parse_toml_spec(&emit_toml(&sample().to_template()).unwrap()),
        expected
    );
    assert_eq!(
        parse_hcl_spec(&emit_hcl(&sample().to_template()).unwrap()),
        expected
    );
}
