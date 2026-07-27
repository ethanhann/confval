//! Round-trip tests for the emit half of the write path.
//!
//! For each format, a spec emits to text and parses back, and the reparsed spec
//! compares equal. `PartialEq` on `Located` ignores the span, so the comparison
//! is by value. The first two tests populate a spec and cover scalars, a filled
//! optional block, a nested list that groups into an array of tables, and a
//! string list. Later tests add deep nesting and an emit-alone round trip that
//! carries a parsed `Map`, a shape populate never produces. The per-shape and
//! error-path assertions live in the frontend modules' unit tests.

use confval::format::hcl::{emit_hcl, parse_hcl};
use confval::format::toml::{emit_toml, parse_toml, parse_toml_fields};
use confval::format::{Fields, FromFields};
use confval::prelude::*;

#[derive(confval::Spec, PartialEq, Debug)]
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

#[derive(confval::Spec, PartialEq, Debug)]
struct ServiceSpec {
    port: Located<i64>,
}

impl Validate for ServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
    #[confval(default = 4)]
    workers: Located<i64>,
    allow: Vec<Located<String>>,
    #[confval(nested, default)]
    limits: Option<Located<LimitsSpec>>,
    #[confval(nested)]
    service: Vec<Located<ServiceSpec>>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// A source spec whose `limits` is absent, so populate fills it, and which
/// carries a two-element string list and two nested-list blocks.
fn sample() -> ServerSpec {
    ServerSpec {
        hostname: Located::detached("127.0.0.1".to_string()),
        port: Located::detached(8080),
        workers: Located::detached(4),
        allow: vec![
            Located::detached("10.0.0.0/8".to_string()),
            Located::detached("192.168.0.0/16".to_string()),
        ],
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

fn populated_of(fields: &Fields) -> ServerSpec {
    let mut report = Report::new();
    let Some(spec) = ServerSpec::from_fields(fields, &mut report) else {
        panic!("populated fields should parse");
    };
    assert!(
        !report.has_issues(),
        "populate issues: {:?}",
        report.issues()
    );
    spec
}

fn parse_toml_spec(text: &str) -> ServerSpec {
    let mut sources = SourceMap::new();
    let id = sources.add("emitted.toml", text.to_string());
    let mut report = Report::new();
    let Some(spec) = parse_toml::<ServerSpec>(&sources, id, &mut report) else {
        panic!("emitted toml should parse");
    };
    assert!(
        !report.has_issues(),
        "reparse issues: {:?}",
        report.issues()
    );
    spec
}

fn parse_hcl_spec(text: &str) -> ServerSpec {
    let mut sources = SourceMap::new();
    let id = sources.add("emitted.hcl", text.to_string());
    let mut report = Report::new();
    let Some(spec) = parse_hcl::<ServerSpec>(&sources, id, &mut report) else {
        panic!("emitted hcl should parse");
    };
    assert!(
        !report.has_issues(),
        "reparse issues: {:?}",
        report.issues()
    );
    spec
}

#[test]
fn toml_round_trips_a_populated_spec() {
    // Arrange
    let fields = sample().to_fields();
    let populated = populated_of(&fields);
    // Act
    let text = emit_toml(&fields).expect("emit toml");
    let reparsed = parse_toml_spec(&text);
    // Assert
    assert!(
        text.contains("[[service]]"),
        "nested list should group: {text}"
    );
    assert_eq!(reparsed, populated);
    assert_eq!(
        reparsed.limits.as_ref().unwrap().value.max_body_mb.value,
        16
    );
    assert_eq!(reparsed.service.len(), 2);
}

#[test]
fn hcl_round_trips_a_populated_spec() {
    // Arrange
    let fields = sample().to_fields();
    let populated = populated_of(&fields);
    // Act
    let text = emit_hcl(&fields).expect("emit hcl");
    let reparsed = parse_hcl_spec(&text);
    // Assert
    assert_eq!(
        text.matches("service {").count(),
        2,
        "nested list should be repeated blocks: {text}"
    );
    assert_eq!(reparsed, populated);
    assert_eq!(
        reparsed.limits.as_ref().unwrap().value.mode.value,
        "enforce"
    );
    assert_eq!(reparsed.service.len(), 2);
}

#[derive(confval::Spec, PartialEq, Debug)]
#[confval(derive_default)]
struct DeepLeaf {
    #[confval(default = 7)]
    n: Located<i64>,
}

impl Validate for DeepLeaf {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
#[confval(derive_default)]
struct DeepMid {
    #[confval(nested, default)]
    leaf: Option<Located<DeepLeaf>>,
}

impl Validate for DeepMid {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct DeepTop {
    #[confval(nested, default)]
    mid: Option<Located<DeepMid>>,
}

impl Validate for DeepTop {
    fn validate(&self, _report: &mut Report) {}
}

fn deep_populated(fields: &Fields) -> DeepTop {
    let mut report = Report::new();
    let Some(spec) = DeepTop::from_fields(fields, &mut report) else {
        panic!("deep fields should parse");
    };
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

fn parse_deep<F>(text: &str, parse: F) -> DeepTop
where
    F: Fn(&SourceMap, confval::source::SourceId, &mut Report) -> Option<DeepTop>,
{
    let mut sources = SourceMap::new();
    let id = sources.add("deep", text.to_string());
    let mut report = Report::new();
    let Some(spec) = parse(&sources, id, &mut report) else {
        panic!("emitted deep config should parse");
    };
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

#[test]
fn deep_nesting_round_trips_in_both_formats() {
    // A three-level fill exercises nested blocks: `[mid]` then `[mid.leaf]` in
    // TOML, and a block inside a block in HCL.
    let fields = DeepTop { mid: None }.to_fields();
    let populated = deep_populated(&fields);

    let toml = emit_toml(&fields).expect("emit toml");
    assert!(toml.contains("[mid.leaf]"), "dotted header: {toml}");
    assert_eq!(parse_deep(&toml, parse_toml::<DeepTop>), populated);

    let hcl = emit_hcl(&fields).expect("emit hcl");
    assert!(hcl.contains("    n = 7"), "indented two levels: {hcl}");
    assert_eq!(parse_deep(&hcl, parse_hcl::<DeepTop>), populated);
}

#[derive(confval::Spec, PartialEq, Debug)]
struct TlsSpec {
    cert: Located<String>,
}

impl Validate for TlsSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct TlsHolder {
    #[confval(nested)]
    tls: Option<Located<TlsSpec>>,
}

impl Validate for TlsHolder {
    fn validate(&self, _report: &mut Report) {}
}

fn parse_holder(text: &str) -> TlsHolder {
    let mut sources = SourceMap::new();
    let id = sources.add("holder.toml", text.to_string());
    let mut report = Report::new();
    let Some(spec) = parse_toml::<TlsHolder>(&sources, id, &mut report) else {
        panic!("holder should parse");
    };
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

#[test]
fn toml_emit_alone_preserves_a_parsed_map() {
    // The source spells `tls` as an inline table, which parses as a `Map`, a
    // shape populate never produces. Emitting the parsed `Fields` and reparsing
    // must reach the same spec, so emit alone inverts parse.
    let source = "tls = { cert = \"a.pem\" }\n";
    let mut sources = SourceMap::new();
    let id = sources.add("in.toml", source.to_string());
    let mut report = Report::new();
    let Some(fields) = parse_toml_fields(&sources, id, &mut report) else {
        panic!("source should parse to fields");
    };

    let text = emit_toml(&fields).expect("emit toml");
    let reparsed = parse_holder(&text);
    assert_eq!(reparsed, parse_holder(source));
    assert_eq!(reparsed.tls.as_ref().unwrap().value.cert.value, "a.pem");
}
