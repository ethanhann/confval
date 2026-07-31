//! End-to-end exercise of the KDL frontend through the span-first pipeline: a
//! handwritten Spec and Config pair driven through parse, validation, the
//! error gate, and lowering, plus the KDL-specific mapping behaviors an
//! operator can observe: repeated-node lists, the duplicate report for a
//! repeated scalar, and the write path back to canonical text.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "derive")]

use confval::format::kdl::{emit_kdl, parse_kdl};
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
    let id = sources.add("server.kdl", input);
    let Some(spec) = parse_kdl::<ServerSpec>(&sources, id, &mut report) else {
        return (sources, report, None);
    };
    validate_server_spec(&spec, &mut report);
    if report.has_errors() {
        return (sources, report, None);
    }
    let config = ServerConfig::lower(&spec, &mut report);
    (sources, report, config)
}

const VALID: &str = r#"hostname "127.0.0.1"
port 8080
daemon #false
allow "10.0.0.0/8" "192.168.0.0/16"

tls {
  cert "cert.pem"
  key "key.pem"
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
fn a_repeated_node_list_accumulates_in_document_order() {
    // Arrange
    let input = r#"hostname "127.0.0.1"
port 8080
daemon #false
allow "10.0.0.0/8"
allow "192.168.0.0/16"
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    assert_eq!(config.unwrap().allow, vec!["10.0.0.0/8", "192.168.0.0/16"]);
}

#[test]
fn a_repeated_scalar_reports_a_duplicate_pointing_at_the_first() {
    // Arrange
    let input = "hostname \"a\"\nport 8080\nport 9090\ndaemon #false\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.kdl", input);

    // Act
    let spec = parse_kdl::<ServerSpec>(&sources, id, &mut report);

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
        "port 9090"
    );
    let (first, label) = &issue.related[0];
    assert_eq!(
        &input[first.start as usize..first.end as usize],
        "port 8080"
    );
    assert_eq!(label, "first declared here");
}

#[test]
fn an_interleaved_repeated_node_list_still_accumulates() {
    // Arrange
    // A field between the two occurrences must not split the list.
    let input = r#"hostname "127.0.0.1"
allow "10.0.0.0/8"
port 8080
daemon #false
allow "192.168.0.0/16"
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    assert_eq!(config.unwrap().allow, vec!["10.0.0.0/8", "192.168.0.0/16"]);
}

#[test]
fn a_duplicate_property_reports_like_a_repeated_scalar() {
    // Arrange
    // kdl-rs keeps both entries of a duplicated property, so the pair reaches
    // the walk as two same-named fields and the single-value report fires.
    let input = r#"hostname "127.0.0.1"
port 8080
daemon #false
tls cert="a.pem" cert="b.pem" key="k.pem"
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.kdl", input);

    // Act
    let spec = parse_kdl::<ServerSpec>(&sources, id, &mut report);

    // Assert
    let spec = spec.expect("first occurrence should win");
    let tls = spec.tls.expect("tls should parse");
    assert_eq!(tls.value.cert.value, "a.pem");
    let issue = report
        .issues()
        .iter()
        .find(|issue| issue.message == "duplicate field: cert")
        .expect("the duplicate property must be reported");
    let second = issue.span.unwrap();
    assert_eq!(
        &input[second.start as usize..second.end as usize],
        "cert=\"b.pem\""
    );
}

#[test]
fn emit_alone_inverts_parse_for_the_kdl_only_spellings() {
    // Arrange
    // The repeated-node list and the property block are shapes populate never
    // produces, so this pins that emit inverts parse without populate in the
    // loop.
    let source = r#"hostname "127.0.0.1"
port 8080
daemon #false
allow "10.0.0.0/8"
allow "192.168.0.0/16"
tls cert="cert.pem" key="key.pem"
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("in.kdl", source);
    let fields = confval::format::kdl::parse_kdl_fields(&sources, id, &mut report).unwrap();
    assert!(!report.has_issues(), "issues: {:?}", report.issues());

    // Act
    let text = emit_kdl(&fields).expect("emit kdl");

    // Assert
    let mut emitted_sources = SourceMap::new();
    let emitted_id = emitted_sources.add("out.kdl", text.clone());
    let mut emitted_report = Report::new();
    let from_emitted: ServerSpec =
        parse_kdl(&emitted_sources, emitted_id, &mut emitted_report).unwrap();
    assert!(
        !emitted_report.has_issues(),
        "emitted text should reparse cleanly, got: {text}"
    );
    let mut original_report = Report::new();
    let from_original: ServerSpec = parse_kdl(&sources, id, &mut original_report).unwrap();
    assert_eq!(from_emitted, from_original, "emitted text: {text}");
}

#[test]
fn a_property_spelled_block_parses_like_a_children_block() {
    // Arrange
    let input = r#"hostname "127.0.0.1"
port 8080
daemon #false
tls cert="cert.pem" key="key.pem"
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    assert!(config.is_some());
}

#[test]
fn all_problems_are_reported_in_one_pass() {
    // Arrange
    // The type mismatch is on an optional field so the tree still builds and
    // validation runs. A mismatch on a required field stops the entity at
    // structural errors.
    let input = r#"hostname "127.0.0.1"
port 99999
daemon #false
allow #true
hostnme "typo"
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
    let input = "hostname \"127.0.0.1\"\n";

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
fn missing_field_in_nested_block_reports_inside_the_block() {
    // Arrange
    let input = r#"hostname "127.0.0.1"
port 8080
daemon #false

tls {
  cert "cert.pem"
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
        .expect("missing key in tls block must be reported");
    let span = issue
        .span
        .expect("missing-field errors carry the enclosing span");
    // The enclosing span sits inside the children braces.
    let brace = input.find('{').unwrap();
    assert!(span.start as usize > brace);
}

#[test]
fn syntax_errors_report_one_issue_per_diagnostic_with_spans() {
    // Arrange
    // `=` between a node name and a value is not KDL.
    let input = "port = 8080\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("broken.kdl", input);

    // Act
    let parsed = parse_kdl::<ServerSpec>(&sources, id, &mut report);

    // Assert
    assert!(parsed.is_none());
    assert!(report.has_errors());
    for issue in report.issues() {
        assert!(issue.message.starts_with("syntax error"), "got: {issue:?}");
        assert!(issue.span.is_some(), "diagnostic without a span: {issue:?}");
    }
}

#[test]
fn a_populated_spec_round_trips_through_emitted_kdl() {
    // Arrange
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.kdl", VALID);
    let spec: ServerSpec = parse_kdl(&sources, id, &mut report).unwrap();
    assert!(!report.has_issues(), "issues: {:?}", report.issues());

    // Act
    let text = emit_kdl(&spec.to_fields()).expect("emit kdl");

    // Assert
    let mut reparse_sources = SourceMap::new();
    let reparse_id = reparse_sources.add("round.kdl", text.clone());
    let mut reparse_report = Report::new();
    let round: ServerSpec = parse_kdl(&reparse_sources, reparse_id, &mut reparse_report).unwrap();
    assert!(
        !reparse_report.has_issues(),
        "emitted text should reparse cleanly, got: {text}"
    );
    // Populate fills `limits` from its default, so the reparse compares
    // against the populated form of the original.
    let mut populate_report = Report::new();
    let populated = ServerSpec::from_fields(&spec.to_fields(), &mut populate_report).unwrap();
    assert_eq!(round, populated);
}

#[test]
fn a_template_renders_absent_optional_fields_as_slashdash_entries() {
    // Arrange
    let input = "hostname \"127.0.0.1\"\nport 8080\ndaemon #false\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.kdl", input);
    let spec: ServerSpec = parse_kdl(&sources, id, &mut report).unwrap();

    // Act
    let text = emit_kdl(&spec.to_template()).expect("emit kdl template");

    // Assert
    // The unset optional list is a bare slashdash node, and the unmarked
    // optional block is a slashdash block.
    assert!(text.contains("/-allow\n"), "got:\n{text}");
    assert!(text.contains("/-tls {"), "got:\n{text}");
    // The template still parses to the same spec.
    let mut round_sources = SourceMap::new();
    let round_id = round_sources.add("round.kdl", text.clone());
    let mut round_report = Report::new();
    let round: ServerSpec = parse_kdl(&round_sources, round_id, &mut round_report)
        .unwrap_or_else(|| panic!("template should parse: {text}"));
    assert!(round.allow.is_none());
    assert!(round.tls.is_none());
}

#[test]
fn a_template_renders_the_doc_chain_as_line_comments() {
    // Arrange
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.kdl", VALID);
    let spec: ServerSpec = parse_kdl(&sources, id, &mut report).unwrap();

    // Act
    let text = emit_kdl(&spec.to_template()).expect("emit kdl template");

    // Assert
    // The nested field's doc renders as a `//` comment at its indentation.
    assert!(
        text.contains("  // The maximum request body size, in megabytes."),
        "got:\n{text}"
    );
}
