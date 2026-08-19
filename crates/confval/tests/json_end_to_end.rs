//! End-to-end exercise of the JSON frontend through the span-first pipeline.
//! A derived Spec and Config pair runs through parse, validation, the error
//! gate, and lowering.
//!
//! The suite also covers the JSON-specific mapping behaviors an operator can
//! observe: duplicate keys resolved by the spec's declared shape, the root
//! object requirement, and the write path back to canonical text.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "derive")]

use confval::format::json::{emit_json, parse_json, parse_json_fields};
use confval::format::{FromFields, ToFields};
use confval::prelude::{Located, Lower, Report, SourceMap, Validate};
use std::net::SocketAddr;

#[derive(Debug, PartialEq, confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
    daemon: Located<bool>,
    allow: Option<Located<Vec<Located<String>>>>,
    #[confval(nested)]
    tls: Option<Located<TlsSpec>>,
    #[confval(nested, default)]
    limits: Located<LimitsSpec>,
}

#[derive(Debug, PartialEq, confval::Spec)]
struct TlsSpec {
    cert: Located<String>,
    key: Located<String>,
}

#[derive(Debug, PartialEq, confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    /// The maximum request body size, in megabytes.
    #[confval(default = 10)]
    max_body_mb: Located<i64>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for TlsSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn validate_server_spec(spec: &ServerSpec, report: &mut Report) {
    if !(1..=65535).contains(&spec.port.value) {
        report
            .error(format!("port out of range: {}", spec.port.value))
            .at(spec.port.span)
            .emit();
    }
}

#[derive(Debug, confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    #[confval(lower(from = (hostname, port), with = parse_addr))]
    addr: SocketAddr,
    daemon: bool,
    #[confval(lower(from = allow, with = allow_to_vec))]
    allow: Vec<String>,
    #[confval(nested)]
    tls: Option<TlsConfig>,
    #[confval(nested)]
    limits: LimitsConfig,
}

#[derive(Debug, confval::Config)]
#[confval(lower_from = TlsSpec)]
struct TlsConfig {
    cert: String,
    key: String,
}

#[derive(Debug, confval::Config)]
#[confval(lower_from = LimitsSpec)]
struct LimitsConfig {
    max_body_mb: i64,
}

fn parse_addr(
    hostname: &Located<String>,
    port: &Located<i64>,
    report: &mut Report,
) -> Option<SocketAddr> {
    match format!("{}:{}", hostname.value, port.value).parse() {
        Ok(addr) => Some(addr),
        Err(_) => {
            report
                .error(format!(
                    "invalid address: {}:{}",
                    hostname.value, port.value
                ))
                .at(hostname.span)
                .emit();
            None
        }
    }
}

fn allow_to_vec(
    value: &Option<Located<Vec<Located<String>>>>,
    _report: &mut Report,
) -> Option<Vec<String>> {
    Some(
        value
            .as_ref()
            .map(|list| {
                list.value
                    .iter()
                    .map(|element| element.value.clone())
                    .collect()
            })
            .unwrap_or_default(),
    )
}

fn load(input: &str) -> (SourceMap, Report, Option<ServerConfig>) {
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.json", input);
    let Some(spec) = parse_json::<ServerSpec>(&sources, id, &mut report) else {
        return (sources, report, None);
    };
    validate_server_spec(&spec, &mut report);
    if report.has_errors() {
        return (sources, report, None);
    }
    let config = ServerConfig::lower(&spec, &mut report);
    (sources, report, config)
}

fn spec_of(input: &str) -> ServerSpec {
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.json", input);
    let spec = parse_json::<ServerSpec>(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("{input} should parse, got: {:?}", report.issues()));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

const VALID: &str = r#"{
  "hostname": "127.0.0.1",
  "port": 8080,
  "daemon": false,
  "allow": ["10.0.0.0/8", "192.168.0.0/16"],
  "tls": {
    "cert": "cert.pem",
    "key": "key.pem"
  }
}
"#;

#[test]
fn valid_config_parses_and_lowers() {
    // Act
    let (_, report, config) = load(VALID);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    let config = config.unwrap();
    assert_eq!(config.addr, "127.0.0.1:8080".parse().unwrap());
    assert!(!config.daemon);
    assert_eq!(config.allow, vec!["10.0.0.0/8", "192.168.0.0/16"]);
    let tls = config.tls.unwrap();
    assert_eq!(tls.cert, "cert.pem");
    assert_eq!(tls.key, "key.pem");
    assert_eq!(config.limits.max_body_mb, 10);
}

#[test]
fn all_problems_are_reported_in_one_pass() {
    // Arrange
    // The type mismatch is on an optional field, so the tree still builds and
    // validation still runs.
    let input = r#"{
  "hostname": "127.0.0.1",
  "port": 99999,
  "daemon": false,
  "allow": true,
  "hostnme": "typo"
}
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(config.is_none());
    let messages: Vec<&str> = report
        .issues()
        .iter()
        .map(|issue| issue.message.as_str())
        .collect();
    assert!(
        messages.contains(&"expected array of strings, found bool"),
        "got: {messages:?}"
    );
    assert!(
        messages.contains(&"unknown field: hostnme"),
        "got: {messages:?}"
    );
    assert!(
        messages.contains(&"port out of range: 99999"),
        "got: {messages:?}"
    );
    assert_eq!(messages.len(), 3, "got: {messages:?}");
}

#[test]
fn missing_required_fields_are_all_reported() {
    // Arrange
    let input = r#"{"hostname": "127.0.0.1"}"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(config.is_none());
    let messages: Vec<&str> = report
        .issues()
        .iter()
        .map(|issue| issue.message.as_str())
        .collect();
    assert!(
        messages.contains(&"missing required field: port"),
        "got: {messages:?}"
    );
    assert!(
        messages.contains(&"missing required field: daemon"),
        "got: {messages:?}"
    );
}

#[test]
fn missing_field_in_a_nested_object_reports_inside_its_braces() {
    // Arrange
    let input = r#"{
  "hostname": "127.0.0.1",
  "port": 8080,
  "daemon": false,
  "tls": {
    "cert": "cert.pem"
  }
}
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(config.is_none());
    let issue = report
        .issues()
        .iter()
        .find(|issue| issue.message == "missing required field: key")
        .expect("the missing key in tls must be reported");
    let span = issue
        .span
        .expect("missing-field errors carry the enclosing span");
    // The enclosing span is the nested object's brace range, so the error
    // points inside `tls` rather than at the whole document.
    let text = &input[span.start as usize..span.end as usize];
    assert!(text.starts_with('{'), "got: {text:?}");
    assert!(text.contains("\"cert\""), "got: {text:?}");
    assert!(!text.contains("\"hostname\""), "got: {text:?}");
}

#[test]
fn a_syntax_error_reports_one_issue_at_its_location() {
    // Arrange
    // A trailing comma is a JSONC extension this frontend turns off.
    let input = "{\"port\": 8080,}\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("broken.json", input);

    // Act
    let parsed = parse_json::<ServerSpec>(&sources, id, &mut report);

    // Assert
    assert!(parsed.is_none());
    assert_eq!(report.issues().len(), 1);
    let issue = &report.issues()[0];
    assert_eq!(
        issue.message,
        "syntax error: trailing commas are not allowed"
    );
    let span = issue.span.expect("a syntax error carries its span");
    assert_eq!(&input[span.start as usize..span.end as usize], ",");
}

#[test]
fn a_root_that_is_not_an_object_reports_and_yields_no_tree() {
    // Arrange
    for input in ["[1, 2]", "\"text\"", "42", "true", "null", ""] {
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let id = sources.add("root.json", input);

        // Act
        let parsed = parse_json::<ServerSpec>(&sources, id, &mut report);

        // Assert
        assert!(parsed.is_none(), "input: {input}");
        assert_eq!(
            report.issues()[0].message,
            "expected an object at the document root",
            "input: {input}"
        );
    }
}

#[test]
fn a_duplicated_list_key_accumulates_in_document_order() {
    // Arrange
    let input = r#"{
  "hostname": "127.0.0.1",
  "allow": "10.0.0.0/8",
  "port": 8080,
  "daemon": false,
  "allow": "192.168.0.0/16"
}
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    assert_eq!(config.unwrap().allow, vec!["10.0.0.0/8", "192.168.0.0/16"]);
}

#[test]
fn a_duplicated_scalar_key_reports_a_duplicate_pointing_at_the_first() {
    // Arrange
    let input = r#"{"hostname": "a", "port": 8080, "port": 9090, "daemon": false}"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.json", input);

    // Act
    let spec = parse_json::<ServerSpec>(&sources, id, &mut report);

    // Assert
    // The first occurrence wins, so the spec still builds.
    let spec = spec.expect("first occurrence should win");
    assert_eq!(spec.port.value, 8080);
    let issue = report
        .issues()
        .iter()
        .find(|issue| issue.message == "duplicate field: port")
        .expect("the repeat must be reported");
    let second = issue.span.unwrap();
    assert_eq!(
        &input[second.start as usize..second.end as usize],
        "\"port\": 9090"
    );
    let (first, label) = &issue.related[0];
    assert_eq!(
        &input[first.start as usize..first.end as usize],
        "\"port\": 8080"
    );
    assert_eq!(label, "first declared here");
}

#[test]
fn duplicate_keys_inside_a_nested_object_resolve_the_same_way() {
    // Arrange
    let input = r#"{
  "hostname": "127.0.0.1",
  "port": 8080,
  "daemon": false,
  "tls": {"cert": "a.pem", "cert": "b.pem", "key": "k.pem"}
}
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.json", input);

    // Act
    let spec = parse_json::<ServerSpec>(&sources, id, &mut report);

    // Assert
    let spec = spec.expect("first occurrence should win");
    let tls = spec.tls.expect("tls should parse");
    assert_eq!(tls.value.cert.value, "a.pem");
    let issue = report
        .issues()
        .iter()
        .find(|issue| issue.message == "duplicate field: cert")
        .expect("the nested duplicate must be reported");
    let second = issue.span.unwrap();
    assert_eq!(
        &input[second.start as usize..second.end as usize],
        "\"cert\": \"b.pem\""
    );
}

#[test]
fn a_populated_spec_round_trips_through_emitted_json() {
    // Arrange
    let spec = spec_of(VALID);

    // Act
    let text = emit_json(&spec.to_fields()).expect("emit json");

    // Assert
    let mut sources = SourceMap::new();
    let id = sources.add("round.json", text.clone());
    let mut report = Report::new();
    let round: ServerSpec = parse_json(&sources, id, &mut report).unwrap();
    assert!(
        !report.has_issues(),
        "emitted text should reparse cleanly, got: {text}"
    );
    // Populate fills `limits` from its default, so the reparse compares
    // against the populated form of the original.
    let mut populate_report = Report::new();
    let populated = ServerSpec::from_fields(&spec.to_fields(), &mut populate_report).unwrap();
    assert_eq!(round, populated);
}

#[test]
fn emit_alone_inverts_parse_for_a_parsed_tree() {
    // Arrange
    // No populate in the loop, so this pins that emit inverts parse over the
    // shapes a source wrote rather than the shapes populate produces.
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("in.json", VALID);
    let fields = parse_json_fields(&sources, id, &mut report).unwrap();
    assert!(!report.has_issues(), "issues: {:?}", report.issues());

    // Act
    let text = emit_json(&fields).expect("emit json");

    // Assert
    let mut emitted_sources = SourceMap::new();
    let emitted_id = emitted_sources.add("out.json", text.clone());
    let mut emitted_report = Report::new();
    let from_emitted: ServerSpec =
        parse_json(&emitted_sources, emitted_id, &mut emitted_report).unwrap();
    assert!(
        !emitted_report.has_issues(),
        "emitted text should reparse cleanly, got: {text}"
    );
    assert_eq!(from_emitted, spec_of(VALID), "emitted text: {text}");
}

#[test]
fn a_grouped_duplicate_list_key_reparses_to_the_same_resolved_list() {
    // Arrange
    // Grouping collapses the two members into one array, so the round trip
    // holds at the walk's resolution rather than at the `Fields` level.
    let input = r#"{
  "hostname": "127.0.0.1",
  "port": 8080,
  "daemon": false,
  "allow": "10.0.0.0/8",
  "allow": "192.168.0.0/16"
}
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("in.json", input);
    let fields = parse_json_fields(&sources, id, &mut report).unwrap();

    // Act
    let text = emit_json(&fields).expect("emit json");

    // Assert
    assert!(
        text.contains(r#""allow": ["10.0.0.0/8", "192.168.0.0/16"]"#),
        "got: {text}"
    );
    assert_eq!(spec_of(&text), spec_of(input));
}

#[test]
fn a_grouped_array_occurrence_reparses_to_the_same_resolved_list() {
    // Arrange
    // An array occurrence contributes its elements and a scalar occurrence
    // contributes itself, so the grouped member reads back as the same list
    // the walk accumulates from the original.
    let input = r#"{
  "hostname": "127.0.0.1",
  "port": 8080,
  "daemon": false,
  "allow": ["10.0.0.0/8", "192.168.0.0/16"],
  "allow": "172.16.0.0/12"
}
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("in.json", input);
    let fields = parse_json_fields(&sources, id, &mut report).unwrap();

    // Act
    let text = emit_json(&fields).expect("emit json");

    // Assert
    assert!(
        text.contains(r#""allow": ["10.0.0.0/8", "192.168.0.0/16", "172.16.0.0/12"]"#),
        "got: {text}"
    );
    assert_eq!(spec_of(&text), spec_of(input));
}

#[test]
fn a_grouped_duplicate_scalar_key_trades_its_duplicate_report_for_a_mismatch() {
    // Arrange
    // The grouped member is an array where a scalar is expected, which is the
    // trade grouping makes for never emitting a duplicate key.
    let input = r#"{"hostname": "a", "port": 8080, "port": 9090, "daemon": false}"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("in.json", input);
    let fields = parse_json_fields(&sources, id, &mut report).unwrap();

    // Act
    let text = emit_json(&fields).expect("emit json");

    // Assert
    assert!(text.contains(r#""port": [8080, 9090]"#), "got: {text}");
    let mut round_sources = SourceMap::new();
    let round_id = round_sources.add("round.json", text.clone());
    let mut round_report = Report::new();
    let round = parse_json::<ServerSpec>(&round_sources, round_id, &mut round_report);
    assert!(round.is_none(), "got: {text}");
    let messages: Vec<&str> = round_report
        .issues()
        .iter()
        .map(|issue| issue.message.as_str())
        .collect();
    assert!(
        messages.contains(&"expected integer, found array"),
        "got: {messages:?}"
    );
}

#[test]
fn a_template_emits_the_same_text_as_the_populated_model() {
    // Arrange
    // JSON has no comment syntax, so a template carries nothing the populated
    // model does not.
    let spec = spec_of(r#"{"hostname": "127.0.0.1", "port": 8080, "daemon": false}"#);

    // Act
    let template = emit_json(&spec.to_template()).expect("emit json template");

    // Assert
    assert_eq!(template, emit_json(&spec.to_fields()).expect("emit json"));
    assert!(!template.contains("The maximum request body size"));
    // The unset optional fields are absent rather than shown, so the template
    // parses back to the same spec.
    let mut sources = SourceMap::new();
    let id = sources.add("template.json", template.clone());
    let mut report = Report::new();
    let round: ServerSpec = parse_json(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("the template should parse: {template}"));
    assert!(round.allow.is_none());
    assert!(round.tls.is_none());
}

#[derive(Debug, PartialEq, confval::Spec)]
struct FleetSpec {
    #[confval(nested)]
    service: Vec<Located<ServiceSpec>>,
}

#[derive(Debug, PartialEq, confval::Spec)]
struct ServiceSpec {
    name: Located<String>,
}

impl Validate for FleetSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_repeated_block_with_one_instance_round_trips() {
    // Arrange
    // With one instance the emitter writes a bare object rather than a
    // one-element array, so the parser must read that form back as a
    // one-element list.
    let fleet = FleetSpec {
        service: vec![Located::detached(ServiceSpec {
            name: Located::detached("api".to_string()),
        })],
    };
    let text = emit_json(&fleet.to_fields()).expect("emit json");
    let mut sources = SourceMap::new();
    let id = sources.add("one.json", text.clone());
    let mut report = Report::new();

    // Act
    let round: Option<FleetSpec> = parse_json(&sources, id, &mut report);

    // Assert
    assert!(
        !report.has_issues(),
        "the emitted text should reparse cleanly, got: {text}\nissues: {:?}",
        report.issues()
    );
    let round = round.unwrap();
    assert_eq!(round.service.len(), 1);
    assert_eq!(round.service[0].value.name.value, "api");
}
