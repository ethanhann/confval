//! The narrow helpers reach an operator through a derived `Config`, so these
//! drive a value through the generated `Lower` impl end to end. A
//! `with = narrow::i64_to_u16` field that overflows fails lowering and reports
//! at the source span, the chain a unit test on the helper alone does not cover.

use confval::format::toml::parse_toml;
use confval::prelude::*;

#[derive(confval::Spec, Debug)]
struct ServerSpec {
    port: Located<i64>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Config, Debug)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
}

fn parse(text: &str) -> ServerSpec {
    let mut sources = SourceMap::new();
    let id = sources.add("server.toml", text.to_string());
    let mut report = Report::new();
    let spec = parse_toml::<ServerSpec>(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("fixture should parse: {:?}", report.issues()));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    spec
}

#[test]
fn an_in_range_port_lowers_to_u16() {
    // Arrange
    let spec = parse("port = 8080\n");
    let mut report = Report::new();

    // Act
    let config = ServerConfig::lower(&spec, &mut report);

    // Assert
    assert_eq!(config.expect("an in-range port lowers").port, 8080_u16);
    assert!(!report.has_errors());
}

#[test]
fn an_out_of_range_port_fails_lowering_and_reports_at_the_span() {
    // Arrange
    let spec = parse("port = 70000\n");
    let port_span = spec.port.span;
    let mut report = Report::new();

    // Act
    let config = ServerConfig::lower(&spec, &mut report);

    // Assert
    assert!(config.is_none(), "an out-of-range port fails lowering");
    assert_eq!(
        report.issues()[0].message,
        "value 70000 is out of range for u16"
    );
    assert_eq!(report.issues()[0].span, Some(port_span));
}
