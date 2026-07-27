//! Round-trip tests for the emit half of the write path.
//!
//! For each format, a populated spec emits to text and parses back, and the
//! reparsed spec compares equal to the populated one. `PartialEq` on `Located`
//! ignores the span, so the comparison is by value. This exercises the whole
//! mapping: scalars, a filled optional block, a nested list that groups into an
//! array of tables, and a string list.

use confval::format::hcl::{emit_hcl, parse_hcl};
use confval::format::toml::{emit_toml, parse_toml};
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
