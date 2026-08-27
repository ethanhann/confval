//! End-to-end exercise of the TOML frontend through the span-first pipeline.
//! A derived Spec and Config pair runs through parse, validation, the error
//! gate, and lowering.
//!
//! The suite also covers the TOML-specific mapping behaviors an operator can
//! observe: the three ways to write a nested spec (section, inline table,
//! dotted keys), the array-of-tables form of a repeated block, the
//! parser-level duplicate-key rejection, and the write path back to canonical
//! text.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "derive")]

use confval::format::toml::{emit_toml, parse_toml};
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
    let id = sources.add("server.toml", input);
    let Some(spec) = parse_toml::<ServerSpec>(&sources, id, &mut report) else {
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
    let id = sources.add("server.toml", input);
    let spec = parse_toml::<ServerSpec>(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("{input} should parse, got: {:?}", report.issues()));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

const VALID: &str = r#"hostname = "127.0.0.1"
port = 8080
daemon = false
allow = ["10.0.0.0/8", "192.168.0.0/16"]

[tls]
cert = "cert.pem"
key = "key.pem"
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
    let input = r#"hostname = "127.0.0.1"
port = 99999
daemon = false
allow = true
hostnme = "typo"
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
fn a_type_mismatch_reports_at_the_value_span() {
    // Arrange
    let input = "hostname = \"127.0.0.1\"\nport = 8080\ndaemon = \"yes\"\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", input);

    // Act
    let parsed = parse_toml::<ServerSpec>(&sources, id, &mut report);

    // Assert
    assert!(parsed.is_none());
    let issue = report
        .issues()
        .iter()
        .find(|issue| issue.message == "expected bool, found string")
        .expect("the mismatch must be reported");
    let span = issue.span.expect("a mismatch carries the value span");
    assert_eq!(&input[span.start as usize..span.end as usize], "\"yes\"");
}

#[test]
fn a_native_datetime_surfaces_as_a_type_mismatch() {
    // Arrange
    // The neutral model has no datetime scalar, so TOML's native form must
    // report rather than silently coerce.
    let input = "hostname = 1979-05-27T07:32:00Z\nport = 8080\ndaemon = false\n";

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
        messages.contains(&"expected string, found datetime"),
        "got: {messages:?}"
    );
}

#[test]
fn missing_required_fields_are_all_reported() {
    // Arrange
    let input = "hostname = \"127.0.0.1\"\n";

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
fn an_empty_document_is_an_empty_root_table_not_a_root_error() {
    // Arrange
    // TOML's document root is always a table, so an empty file reaches the
    // field walk and reports its missing fields rather than a root-shape
    // error.
    let (_, report, config) = load("");

    // Assert
    assert!(config.is_none());
    let messages: Vec<&str> = report
        .issues()
        .iter()
        .map(|issue| issue.message.as_str())
        .collect();
    assert!(
        messages.contains(&"missing required field: hostname"),
        "got: {messages:?}"
    );
}

#[test]
fn missing_field_in_a_nested_section_reports_inside_the_section() {
    // Arrange
    let input = r#"hostname = "127.0.0.1"
port = 8080
daemon = false

[tls]
cert = "cert.pem"
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
    // toml_edit's table span covers the header, so the error points at
    // `[tls]` rather than at the whole document.
    let text = &input[span.start as usize..span.end as usize];
    assert_eq!(text, "[tls]");
}

#[test]
fn a_syntax_error_reports_one_issue_at_its_location() {
    // Arrange
    let input = "port = \n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("broken.toml", input);

    // Act
    let parsed = parse_toml::<ServerSpec>(&sources, id, &mut report);

    // Assert
    assert!(parsed.is_none());
    assert_eq!(report.issues().len(), 1);
    let issue = &report.issues()[0];
    assert!(issue.message.starts_with("syntax error:"), "got: {issue:?}");
    assert!(issue.span.is_some(), "a syntax error carries its span");
}

#[test]
fn a_duplicate_key_is_a_parser_level_syntax_error() {
    // Arrange
    // TOML's grammar forbids redefining a key or a table, so toml_edit rejects
    // the document during parsing and from_fields never sees the repeat. No
    // format-level duplicate report exists in TOML.
    let inputs = [
        "port = 8080\nport = 9090\n",
        "[tls]\ncert = \"a.pem\"\n[tls]\nkey = \"a.key\"\n",
    ];
    for input in inputs {
        let mut sources = SourceMap::new();
        let mut report = Report::new();
        let id = sources.add("dup.toml", input);

        // Act
        let parsed = parse_toml::<ServerSpec>(&sources, id, &mut report);

        // Assert
        assert!(parsed.is_none(), "input: {input}");
        let issue = &report.issues()[0];
        assert!(
            issue.message.starts_with("syntax error:"),
            "input: {input}, got: {issue:?}"
        );
        assert!(
            issue.message.contains("duplicate key"),
            "input: {input}, got: {issue:?}"
        );
    }
}

#[test]
fn an_inline_table_lowers_like_a_section() {
    // Arrange
    let input = r#"hostname = "127.0.0.1"
port = 8080
daemon = false
tls = { cert = "cert.pem", key = "key.pem" }
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    let tls = config.unwrap().tls.unwrap();
    assert_eq!(tls.cert, "cert.pem");
    assert_eq!(tls.key, "key.pem");
}

#[test]
fn dotted_keys_build_the_nested_spec() {
    // Arrange
    let input = r#"hostname = "127.0.0.1"
port = 8080
daemon = false
tls.cert = "cert.pem"
tls.key = "key.pem"
"#;

    // Act
    let (_, report, config) = load(input);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    let tls = config.unwrap().tls.unwrap();
    assert_eq!(tls.cert, "cert.pem");
    assert_eq!(tls.key, "key.pem");
}

#[test]
fn a_populated_spec_round_trips_through_emitted_toml() {
    // Arrange
    let spec = spec_of(VALID);

    // Act
    let text = emit_toml(&spec.to_fields()).expect("emit toml");

    // Assert
    let mut sources = SourceMap::new();
    let id = sources.add("round.toml", text.clone());
    let mut report = Report::new();
    let round: ServerSpec = parse_toml(&sources, id, &mut report).unwrap();
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
fn a_template_carries_the_doc_comment_and_reparses_to_the_populated_spec() {
    // Arrange
    // The comment content and placement rules are in the template suite; this
    // pins that a TOML template parses back with the unset optional fields
    // absent rather than shown.
    let spec = spec_of("hostname = \"127.0.0.1\"\nport = 8080\ndaemon = false\n");

    // Act
    let template = emit_toml(&spec.to_template()).expect("emit toml template");

    // Assert
    assert!(
        template.contains("# The maximum request body size, in megabytes."),
        "got: {template}"
    );
    let mut sources = SourceMap::new();
    let id = sources.add("template.toml", template.clone());
    let mut report = Report::new();
    let round: ServerSpec = parse_toml(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("the template should parse: {template}"));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    assert!(round.allow.is_none());
    assert!(round.tls.is_none());
    assert_eq!(round.limits.value.max_body_mb.value, 10);
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
fn an_array_of_tables_lowers_as_a_repeated_block() {
    // Arrange
    let input = "[[service]]\nname = \"api\"\n\n[[service]]\nname = \"worker\"\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("fleet.toml", input);

    // Act
    let fleet: Option<FleetSpec> = parse_toml(&sources, id, &mut report);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    let fleet = fleet.unwrap();
    assert_eq!(fleet.service.len(), 2);
    assert_eq!(fleet.service[0].value.name.value, "api");
    assert_eq!(fleet.service[1].value.name.value, "worker");
    // Each element keeps the span of its own `[[service]]` header.
    let first = fleet.service[0].span;
    let second = fleet.service[1].span;
    assert_eq!(
        &input[first.start as usize..first.end as usize],
        "[[service]]"
    );
    assert_eq!(first.start as usize, 0);
    assert_eq!(second.start as usize, input.rfind("[[service]]").unwrap());
}

#[test]
fn a_repeated_block_with_one_instance_round_trips() {
    // Arrange
    // With one instance the emitter writes a plain section rather than a
    // one-element array of tables, so the parser must read that form back as
    // a one-element list.
    let fleet = FleetSpec {
        service: vec![Located::detached(ServiceSpec {
            name: Located::detached("api".to_string()),
        })],
    };
    let text = emit_toml(&fleet.to_fields()).expect("emit toml");
    let mut sources = SourceMap::new();
    let id = sources.add("one.toml", text.clone());
    let mut report = Report::new();

    // Act
    let round: Option<FleetSpec> = parse_toml(&sources, id, &mut report);

    // Assert
    assert!(
        text.contains("[service]") && !text.contains("[[service]]"),
        "one instance writes a plain section: {text}"
    );
    assert!(
        !report.has_issues(),
        "the emitted text should reparse cleanly, got: {text}\nissues: {:?}",
        report.issues()
    );
    let round = round.unwrap();
    assert_eq!(round.service.len(), 1);
    assert_eq!(round.service[0].value.name.value, "api");
}

confval::keyword_enum!(Mode, {
    Enforce => "enforce",
    Log     => "log",
});

/// A spec whose only rule is a keyword set recorded on a list, so the check the
/// derive generates is the only thing that can report.
#[derive(confval::Spec)]
struct ModesSpec {
    #[confval(default, keywords = Mode)]
    modes: Vec<Located<String>>,
}

impl Validate for ModesSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_parsed_list_reports_a_bad_element_at_that_element() {
    // Arrange
    // The spans come from the parse rather than from a hand-built `Located`, so
    // this pins that a real TOML list keeps one span per element.
    let input = "modes = [\"enforce\", \"shout\"]\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("modes.toml", input);
    let spec: Option<ModesSpec> = parse_toml(&sources, id, &mut report);

    // Act
    spec.expect("the input parses").validate_all(&mut report);

    // Assert
    let issue = &report.issues()[0];
    assert_eq!(issue.message, "unknown value in modes: shout");
    let span = issue.span.expect("the issue carries a span");
    assert_eq!(
        &input[span.start as usize..span.end as usize],
        "\"shout\"",
        "the span underlines the element rather than the list"
    );
}

#[test]
fn an_absent_defaulted_list_reports_nothing() {
    // Arrange
    // A list default is always the empty list, so the recorded check has no
    // value to reject and the defaulted-value branch scalars get is vacuous.
    let input = "";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("empty.toml", input);
    let spec: Option<ModesSpec> = parse_toml(&sources, id, &mut report);

    // Act
    spec.expect("the input parses").validate_all(&mut report);

    // Assert
    assert!(!report.has_issues());
}
