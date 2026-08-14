//! Label references: `#[confval(label)]`, `#[confval(references = <block>)]`, the
//! native-label read for HCL and KDL, and the reference resolution pass.

#![cfg(all(
    feature = "derive",
    feature = "hcl",
    feature = "toml",
    feature = "kdl",
    feature = "json",
    feature = "yaml"
))]

use confval::diagnostic::Report;
use confval::format::{FromFields, hcl, json, kdl, toml, yaml};
use confval::pipeline::{Validate, check_references};
use confval::schema::{Constraint, SchemaType, ToSchema};
use confval::source::{Located, SourceId, SourceMap};

#[derive(confval::Spec)]
struct UpstreamSpec {
    #[confval(label)]
    name: Located<String>,
    host: Located<String>,
    port: Located<i64>,
}

#[derive(confval::Spec)]
struct RuleSpec {
    prefix: Located<String>,
    #[confval(references = upstream)]
    upstream: Located<String>,
}

#[derive(confval::Spec)]
struct GatewaySpec {
    #[confval(nested)]
    upstream: Vec<Located<UpstreamSpec>>,
    #[confval(nested)]
    rules: Vec<Located<RuleSpec>>,
}

impl Validate for UpstreamSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for RuleSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for GatewaySpec {
    fn validate(&self, _report: &mut Report) {}
}

/// The error messages a parse pushed, in order.
fn errors(report: &Report) -> Vec<String> {
    report
        .issues()
        .iter()
        .map(|issue| issue.message.clone())
        .collect()
}

/// Parses a source into the neutral `Fields`, runs `from_fields` for the
/// structural checks (including the unexpected-label report), and runs the
/// reference pass, the way a consumer wires the two together.
fn run(format: &str, sources: &SourceMap, id: SourceId, report: &mut Report) {
    let fields = match format {
        "hcl" => hcl::parse_hcl_fields(sources, id, report),
        "toml" => toml::parse_toml_fields(sources, id, report),
        "kdl" => kdl::parse_kdl_fields(sources, id, report),
        "json" => json::parse_json_fields(sources, id, report),
        "yaml" => yaml::parse_yaml_fields(sources, id, report),
        other => panic!("unknown format {other}"),
    };
    if let Some(fields) = fields {
        let _: Option<GatewaySpec> = GatewaySpec::from_fields(&fields, report);
        check_references(&fields, &GatewaySpec::schema(), report);
    }
}

/// Runs `run` on `text` in `format` and returns the report.
fn parse(format: &str, text: &str) -> Report {
    let mut sources = SourceMap::new();
    let id = sources.add("gateway", text);
    let mut report = Report::new();
    run(format, &sources, id, &mut report);
    report
}

const HCL_RESOLVED: &str = r#"
upstream "api" {
  host = "api.internal"
  port = 8080
}
rules {
  prefix = "/api"
  upstream = "api"
}
"#;

const TOML_RESOLVED: &str = r#"
[[upstream]]
name = "api"
host = "api.internal"
port = 8080

[[rules]]
prefix = "/api"
upstream = "api"
"#;

const KDL_RESOLVED: &str = r#"
upstream "api" {
  host "api.internal"
  port 8080
}
rules {
  prefix "/api"
  upstream "api"
}
"#;

const JSON_RESOLVED: &str = r#"
{
  "upstream": [{ "name": "api", "host": "api.internal", "port": 8080 }],
  "rules": [{ "prefix": "/api", "upstream": "api" }]
}
"#;

const YAML_RESOLVED: &str = r#"
upstream:
  - name: api
    host: api.internal
    port: 8080
rules:
  - prefix: /api
    upstream: api
"#;

#[test]
fn a_resolved_reference_reports_nothing_in_every_format() {
    // Arrange
    let cases = [
        ("hcl", HCL_RESOLVED),
        ("toml", TOML_RESOLVED),
        ("kdl", KDL_RESOLVED),
        ("json", JSON_RESOLVED),
        ("yaml", YAML_RESOLVED),
    ];

    // Act, Assert
    for (format, text) in cases {
        let report = parse(format, text);
        assert!(
            errors(&report).is_empty(),
            "{format}: {:?}",
            errors(&report)
        );
    }
}

#[test]
fn an_undefined_reference_reports_at_the_value_span() {
    // Arrange
    let text = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nrules {\n  prefix = \"/a\"\n  upstream = \"nope\"\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();

    // Act
    run("hcl", &sources, id, &mut report);

    // Assert
    let issue = report
        .issues()
        .iter()
        .find(|i| i.message.contains("upstream"))
        .expect("an undefined-reference error");
    assert_eq!(issue.message, "no upstream named \"nope\"");
    let span = issue.span.expect("the error carries a span");
    assert_eq!(&text[span.start as usize..span.end as usize], "\"nope\"");
    assert!(
        report
            .issues()
            .iter()
            .any(|i| i.help.as_deref().is_some_and(|h| h.contains("api"))),
        "the help lists the defined labels"
    );
}

#[test]
fn an_undefined_reference_reports_in_every_format() {
    // Arrange
    let cases = [
        (
            "hcl",
            HCL_RESOLVED.replace("upstream = \"api\"", "upstream = \"nope\""),
        ),
        (
            "toml",
            TOML_RESOLVED.replace("upstream = \"api\"", "upstream = \"nope\""),
        ),
        (
            "kdl",
            KDL_RESOLVED.replace("upstream \"api\"\n}", "upstream \"nope\"\n}"),
        ),
        (
            "json",
            JSON_RESOLVED.replace("\"upstream\": \"api\"", "\"upstream\": \"nope\""),
        ),
        (
            "yaml",
            YAML_RESOLVED.replace("upstream: api", "upstream: nope"),
        ),
    ];

    // Act, Assert
    for (format, text) in cases {
        let report = parse(format, &text);
        assert!(
            errors(&report)
                .iter()
                .any(|m| m == "no upstream named \"nope\""),
            "{format}: {:?}",
            errors(&report)
        );
    }
}

#[test]
fn a_duplicate_label_reports() {
    // Arrange
    let text = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nupstream \"api\" {\n  host = \"h2\"\n  port = 2\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();

    // Act
    run("hcl", &sources, id, &mut report);

    // Assert
    assert!(
        errors(&report)
            .iter()
            .any(|m| m == "duplicate upstream label \"api\""),
        "got: {:?}",
        errors(&report)
    );
}

#[test]
fn an_empty_label_reports() {
    // Arrange
    let text = "upstream \"\" {\n  host = \"h\"\n  port = 1\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();

    // Act
    run("hcl", &sources, id, &mut report);

    // Assert
    assert!(
        errors(&report)
            .iter()
            .any(|m| m == "a block label must not be empty"),
        "got: {:?}",
        errors(&report)
    );
}

#[test]
fn a_label_on_a_block_that_takes_none_reports() {
    // Arrange
    // RuleSpec designates no label field, so a native label on a rules block is
    // unexpected.
    let text = "rules \"oops\" {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();

    // Act
    run("hcl", &sources, id, &mut report);

    // Assert
    assert!(
        errors(&report)
            .iter()
            .any(|m| m == "this block does not take a label"),
        "got: {:?}",
        errors(&report)
    );
}

#[test]
fn the_schema_records_the_label_field_and_the_reference() {
    // Arrange, Act
    let schema = GatewaySpec::schema();

    // Assert
    let upstream = schema
        .fields
        .iter()
        .find(|f| f.name == "upstream")
        .expect("the upstream block field");
    let SchemaType::Block { schema: block, .. } = &upstream.ty else {
        panic!("upstream should be a block");
    };
    assert!(
        block.fields.iter().any(|f| f.name == "name" && f.label),
        "the name field is the label"
    );
    let rules = schema.fields.iter().find(|f| f.name == "rules").unwrap();
    let SchemaType::Block { schema: rule, .. } = &rules.ty else {
        panic!("rules should be a block");
    };
    let reference = rule.fields.iter().find(|f| f.name == "upstream").unwrap();
    assert_eq!(
        reference.ty,
        SchemaType::Scalar {
            leaf: confval::schema::ScalarType::String,
            constraint: Some(Constraint::References { block: "upstream" }),
        }
    );
}
