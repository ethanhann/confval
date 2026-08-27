//! Label references: `#[confval(label)]`, `#[confval(references = <block>)]`, the
//! native-label read for HCL and KDL, and the reference resolution pass.

use confval::diagnostic::Report;
use confval::format::{FromFields, hcl, json, kdl, toml, yaml};
use confval::pipeline::{Validate, check_references, declares_labeled_block, scope_labels};
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

#[derive(confval::Spec)]
struct TypoRuleSpec {
    #[confval(references = nowhere)]
    upstream: Located<String>,
}

#[derive(confval::Spec)]
struct TypoSpec {
    #[confval(nested)]
    rules: Vec<Located<TypoRuleSpec>>,
}

impl Validate for TypoRuleSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for TypoSpec {
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

/// Parses a source and builds the typed spec, for the tests that assert on the
/// built value rather than the report.
fn build(format: &str, text: &str) -> Option<GatewaySpec> {
    let mut sources = SourceMap::new();
    let id = sources.add("gateway", text);
    let mut report = Report::new();
    let fields = match format {
        "hcl" => hcl::parse_hcl_fields(&sources, id, &mut report),
        "toml" => toml::parse_toml_fields(&sources, id, &mut report),
        "kdl" => kdl::parse_kdl_fields(&sources, id, &mut report),
        "json" => json::parse_json_fields(&sources, id, &mut report),
        "yaml" => yaml::parse_yaml_fields(&sources, id, &mut report),
        other => panic!("unknown format {other}"),
    }?;
    GatewaySpec::from_fields(&fields, &mut report)
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

    for (format, text) in cases {
        // Act
        let report = parse(format, text);

        // Assert
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

    for (format, text) in cases {
        // Act
        let report = parse(format, &text);

        // Assert
        let issue = report
            .issues()
            .iter()
            .find(|i| i.message == "no upstream named \"nope\"")
            .unwrap_or_else(|| panic!("{format}: {:?}", errors(&report)));
        let span = issue.span.unwrap_or_else(|| panic!("{format}: no span"));
        assert!(
            text[span.start as usize..span.end as usize].contains("nope"),
            "{format}: span text {:?}",
            &text[span.start as usize..span.end as usize]
        );
        assert!(
            issue.help.as_deref().is_some_and(|h| h.contains("api")),
            "{format}: help {:?}",
            issue.help
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
    let issue = report
        .issues()
        .iter()
        .find(|i| i.message == "duplicate upstream label \"api\"")
        .expect("a duplicate-label error");
    assert!(
        issue
            .related
            .iter()
            .any(|(_, label)| label == "first declared here"),
        "the duplicate points back at the first declaration: {:?}",
        issue.related
    );
}

#[test]
fn a_reference_to_an_absent_block_reports_a_target_error() {
    // Arrange
    let text = "rules {\n  upstream = \"x\"\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();
    let fields = hcl::parse_hcl_fields(&sources, id, &mut report).expect("the source parses");

    // Act
    check_references(&fields, &TypoSpec::schema(), &mut report);

    // Assert
    let issue = report
        .issues()
        .iter()
        .find(|issue| issue.message == "reference target nowhere is not a labeled block")
        .unwrap_or_else(|| panic!("expected a target error, got: {:?}", errors(&report)));
    let span = issue.span.expect("the error carries a span");
    assert_eq!(&text[span.start as usize..span.end as usize], "\"x\"");
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
            .any(|m| m == "a block label is not allowed here"),
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
        SchemaType::scalar(
            confval::schema::ScalarType::String,
            Some(Constraint::references("upstream"))
        )
    );
}

#[test]
fn a_native_label_and_a_child_label_conflict() {
    // Arrange
    let text = "upstream \"api\" {\n  name = \"api\"\n  host = \"h\"\n  port = 1\n}\nrules {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";

    // Act
    let report = parse("hcl", text);

    // Assert
    let issue = report
        .issues()
        .iter()
        .find(|i| i.message == "a block label is already set")
        .expect("the conflict error");
    assert!(
        issue
            .related
            .iter()
            .any(|(_, label)| label == "the block label"),
        "the conflict points back at the block label: {:?}",
        issue.related
    );
    assert!(
        !errors(&report)
            .iter()
            .any(|m| m.starts_with("no upstream named")),
        "the native label wins so the reference resolves: {:?}",
        errors(&report)
    );
}

#[test]
fn hcl_reads_the_first_label_and_reports_the_extra() {
    // Arrange
    let text = "upstream \"api\" \"extra\" {\n  host = \"h\"\n  port = 1\n}\nrules {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";

    // Act
    let report = parse("hcl", text);

    // Assert
    assert!(
        errors(&report)
            .iter()
            .any(|m| m == "a block label must be the only one"),
        "got: {:?}",
        errors(&report)
    );
    assert!(
        !errors(&report)
            .iter()
            .any(|m| m.starts_with("no upstream named")),
        "the first label resolves: {:?}",
        errors(&report)
    );
}

#[test]
fn kdl_reads_the_first_label_and_reports_the_extra() {
    // Arrange
    let text = "upstream \"api\" \"extra\" {\n  host \"h\"\n  port 1\n}\nrules {\n  prefix \"/a\"\n  upstream \"api\"\n}\n";

    // Act
    let report = parse("kdl", text);

    // Assert
    assert!(
        errors(&report)
            .iter()
            .any(|m| m == "a block label must be the only one"),
        "got: {:?}",
        errors(&report)
    );
    assert!(
        !errors(&report)
            .iter()
            .any(|m| m.starts_with("no upstream named")),
        "the first label resolves: {:?}",
        errors(&report)
    );
}

#[test]
fn kdl_reports_a_non_string_label() {
    // Arrange
    let text = "upstream 8080 {\n  host \"h\"\n  port 1\n}\n";

    // Act
    let report = parse("kdl", text);

    // Assert
    assert!(
        errors(&report)
            .iter()
            .any(|m| m == "a block label must be a string"),
        "got: {:?}",
        errors(&report)
    );
}

#[test]
fn a_missing_label_field_reports_the_ordinary_missing_field_error() {
    // Arrange
    let text = "[[upstream]]\nhost = \"h\"\nport = 1\n";

    // Act
    let report = parse("toml", text);

    // Assert
    assert!(
        errors(&report)
            .iter()
            .any(|m| m == "missing required field: name"),
        "got: {:?}",
        errors(&report)
    );
}

#[test]
fn an_undefined_reference_with_no_blocks_states_the_file_defines_none() {
    // Arrange
    let text = "rules {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";

    // Act
    let report = parse("hcl", text);

    // Assert
    let issue = report
        .issues()
        .iter()
        .find(|i| i.message == "no upstream named \"api\"")
        .expect("an undefined-reference error");
    assert_eq!(issue.help.as_deref(), Some("the file defines no upstream"));
}

#[test]
fn from_fields_reads_the_label_from_the_native_slot_in_hcl() {
    // Arrange
    let text = HCL_RESOLVED;

    // Act
    let spec = build("hcl", text).expect("the spec builds");

    // Assert
    assert_eq!(spec.upstream[0].value.name.value.as_str(), "api");
}

#[test]
fn from_fields_reads_the_label_from_the_child_field_in_toml() {
    // Arrange
    let text = TOML_RESOLVED;

    // Act
    let spec = build("toml", text).expect("the spec builds");

    // Assert
    assert_eq!(spec.upstream[0].value.name.value.as_str(), "api");
}

#[test]
fn scope_labels_collects_labels_with_spans_and_no_report() {
    // Arrange
    let text = HCL_RESOLVED;
    let mut sources = SourceMap::new();
    let id = sources.add("gateway", text);
    let mut report = Report::new();
    let fields = hcl::parse_hcl_fields(&sources, id, &mut report).expect("the source parses");
    let schema = GatewaySpec::schema();

    // Act
    let upstreams = scope_labels(&fields, &schema, "upstream");

    // Assert
    assert!(
        declares_labeled_block(&schema, "upstream"),
        "the root declares the labeled upstream block"
    );
    assert_eq!(upstreams.len(), 1);
    assert_eq!(upstreams[0].value.as_str(), "api");
    assert!(
        text[upstreams[0].span.start as usize..upstreams[0].span.end as usize].contains("api"),
        "the label carries its span: {:?}",
        &text[upstreams[0].span.start as usize..upstreams[0].span.end as usize]
    );
    assert!(errors(&report).is_empty(), "the accessor emits nothing");
}

#[test]
fn a_reference_resolves_against_a_block_defined_later() {
    // Arrange
    let text = "rules {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\nupstream \"api\" {\n  host = \"h\"\n  port = 1\n}\n";

    // Act
    let report = parse("hcl", text);

    // Assert
    assert!(
        errors(&report).is_empty(),
        "the reference resolves against the later block: {:?}",
        errors(&report)
    );
}

#[test]
fn a_duplicate_label_reports_through_the_child_field() {
    // Arrange
    let text = "[[upstream]]\nname = \"api\"\nhost = \"h\"\nport = 1\n\n[[upstream]]\nname = \"api\"\nhost = \"h2\"\nport = 2\n";

    // Act
    let report = parse("toml", text);

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
fn an_empty_label_reports_through_the_child_field() {
    // Arrange
    let text = "[[upstream]]\nname = \"\"\nhost = \"h\"\nport = 1\n";

    // Act
    let report = parse("toml", text);

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
fn a_native_label_round_trips_in_hcl() {
    // Arrange
    let text = "upstream \"api\" {\n  host = \"api.internal\"\n  port = 8080\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();
    let fields = hcl::parse_hcl_fields(&sources, id, &mut report).expect("the source parses");

    // Act
    let out = hcl::emit_hcl(&fields).expect("the fields emit");

    // Assert
    assert!(
        out.contains("upstream \"api\""),
        "the native label survives the round trip: {out}"
    );
}

#[test]
fn a_native_label_round_trips_in_kdl() {
    // Arrange
    let text = "upstream \"api\" {\n  host \"api.internal\"\n  port 8080\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.kdl", text);
    let mut report = Report::new();
    let fields = kdl::parse_kdl_fields(&sources, id, &mut report).expect("the source parses");

    // Act
    let out = kdl::emit_kdl(&fields).expect("the fields emit");

    // Assert
    assert!(
        out.contains("upstream \"api\""),
        "the native label survives the round trip: {out}"
    );
}

#[derive(confval::Spec)]
struct MeshUpstreamSpec {
    #[confval(label)]
    name: Located<String>,
    port: Located<i64>,
}

#[derive(confval::Spec)]
struct MeshPoolSpec {
    #[confval(label)]
    id: Located<String>,
}

#[derive(confval::Spec)]
struct MeshRouteSpec {
    prefix: Option<Located<String>>,
    #[confval(references = upstreams)]
    upstream: Option<Located<String>>,
    #[confval(references = pool)]
    pool: Option<Located<String>>,
}

#[derive(confval::Spec)]
struct MeshServiceSpec {
    name: Located<String>,
    #[confval(nested)]
    routes: Vec<Located<MeshRouteSpec>>,
    #[confval(nested)]
    upstreams: Vec<Located<MeshUpstreamSpec>>,
    #[confval(nested)]
    pool: Vec<Located<MeshPoolSpec>>,
}

#[derive(confval::Spec)]
struct MeshSpec {
    #[confval(nested)]
    services: Vec<Located<MeshServiceSpec>>,
    #[confval(nested)]
    pool: Vec<Located<MeshPoolSpec>>,
}

impl Validate for MeshUpstreamSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MeshPoolSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MeshRouteSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MeshServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MeshSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// Parses HCL text and runs the reference pass against the nested mesh schema,
/// whose labeled blocks sit below the root.
fn mesh_report(text: &str) -> Report {
    let mut sources = SourceMap::new();
    let id = sources.add("mesh", text);
    let mut report = Report::new();
    let Some(fields) = hcl::parse_hcl_fields(&sources, id, &mut report) else {
        panic!("the source parses");
    };
    check_references(&fields, &MeshSpec::schema(), &mut report);
    report
}

/// Two services that each define an upstream and a route naming it. The label
/// `u1` repeats across the sibling services.
const MESH_SIBLINGS: &str = "services {\n  name = \"a\"\n  upstreams \"u1\" {\n    port = 1\n  }\n  routes {\n    upstream = \"u1\"\n  }\n}\nservices {\n  name = \"b\"\n  upstreams \"u1\" {\n    port = 2\n  }\n  routes {\n    upstream = \"u1\"\n  }\n}\n";

#[test]
fn a_sibling_scoped_reference_resolves_within_its_own_service() {
    // Arrange
    // Each route names the upstream of its own service. The target block sits
    // below the root, so a root-level-only model cannot resolve it.
    let text = MESH_SIBLINGS;

    // Act
    let report = mesh_report(text);

    // Assert
    assert!(
        errors(&report).is_empty(),
        "both routes resolve in their own scope: {:?}",
        errors(&report)
    );
}

#[test]
fn the_same_label_in_two_sibling_scopes_is_legal() {
    // Arrange
    // Both services define an upstream labeled `u1`. The duplicate check is
    // scope-local, so the repetition across siblings reports nothing.
    let text = MESH_SIBLINGS;

    // Act
    let report = mesh_report(text);

    // Assert
    assert!(
        !errors(&report)
            .iter()
            .any(|message| message.contains("duplicate")),
        "sibling scopes may reuse a label: {:?}",
        errors(&report)
    );
}

#[test]
fn a_reference_to_another_siblings_label_does_not_resolve() {
    // Arrange
    // Service `b` names service `a`'s upstream. The reference resolves against
    // its own service's upstreams, so it must not see the sibling's label.
    let text = "services {\n  name = \"a\"\n  upstreams \"ua\" {\n    port = 1\n  }\n}\nservices {\n  name = \"b\"\n  upstreams \"ub\" {\n    port = 2\n  }\n  routes {\n    upstream = \"ua\"\n  }\n}\n";

    // Act
    let report = mesh_report(text);

    // Assert
    let messages = errors(&report);
    assert_eq!(messages, vec!["no upstreams named \"ua\"".to_string()]);
    let issue = &report.issues()[0];
    assert_eq!(issue.help.as_deref(), Some("defined upstreams: ub"));
}

#[test]
fn a_repeated_block_name_resolves_at_the_nearest_scope() {
    // Arrange
    // `pool` is declared at the root and inside a service. The route's
    // reference resolves at the nearest declaring scope, the service, so the
    // root's `shared` label is out of reach.
    let text = "pool \"shared\" {\n}\nservices {\n  name = \"a\"\n  pool \"local\" {\n  }\n  routes {\n    pool = \"shared\"\n  }\n}\n";

    // Act
    let report = mesh_report(text);

    // Assert
    let messages = errors(&report);
    assert_eq!(messages, vec!["no pool named \"shared\"".to_string()]);
    let issue = &report.issues()[0];
    assert_eq!(issue.help.as_deref(), Some("defined pool: local"));
}

#[test]
fn a_reference_resolves_in_the_nearest_scope_that_declares_the_target() {
    // Arrange
    let text = "pool \"shared\" {\n}\nservices {\n  name = \"a\"\n  pool \"local\" {\n  }\n  routes {\n    pool = \"local\"\n  }\n}\n";

    // Act
    let report = mesh_report(text);

    // Assert
    assert!(
        errors(&report).is_empty(),
        "the service's own pool resolves: {:?}",
        errors(&report)
    );
}

#[test]
fn a_duplicate_label_within_one_scope_reports() {
    // Arrange
    let text = "services {\n  name = \"a\"\n  upstreams \"u1\" {\n    port = 1\n  }\n  upstreams \"u1\" {\n    port = 2\n  }\n}\n";

    // Act
    let report = mesh_report(text);

    // Assert
    let messages = errors(&report);
    assert_eq!(
        messages,
        vec!["duplicate upstreams label \"u1\"".to_string()]
    );
}

#[derive(confval::Spec)]
struct ScopedRootSpec {
    #[confval(references = pool)]
    active_pool: Located<String>,
    #[confval(nested)]
    services: Vec<Located<ScopedServicesSpec>>,
}

#[derive(confval::Spec)]
struct ScopedServicesSpec {
    name: Located<String>,
    #[confval(nested)]
    pool: Vec<Located<ScopedPoolSpec>>,
}

#[derive(confval::Spec)]
struct ScopedPoolSpec {
    #[confval(label)]
    id: Located<String>,
    size: Located<i64>,
}

impl Validate for ScopedRootSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ScopedServicesSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ScopedPoolSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_reference_out_of_scope_names_scoping_rather_than_the_target() {
    // Arrange
    // `pool` is a labeled block, but only inside `services`, so a root-level
    // reference has no enclosing scope that declares it. The message names
    // scoping as the cause rather than calling the target unlabeled.
    let text =
        "active_pool = \"a\"\nservices {\n  name = \"svc\"\n  pool \"a\" {\n    size = 1\n  }\n}\n";
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();
    let fields = hcl::parse_hcl_fields(&sources, id, &mut report).expect("the source parses");

    // Act
    check_references(&fields, &ScopedRootSpec::schema(), &mut report);

    // Assert
    let issue = report
        .issues()
        .iter()
        .find(|issue| issue.message == "no pool is in scope at this reference")
        .unwrap_or_else(|| panic!("expected the scoping error, got: {:?}", report.issues()));
    assert_eq!(
        issue.help.as_deref(),
        Some(
            "pool is declared in a nested scope, and a reference resolves outward through its enclosing blocks"
        )
    );
    let span = issue.span.expect("the error carries a span");
    assert_eq!(&text[span.start as usize..span.end as usize], "\"a\"");
}

#[derive(confval::Spec)]
struct AlphaBlockSpec {
    #[confval(label)]
    alpha_name: Located<String>,
    value: Located<i64>,
}

#[derive(confval::Spec)]
struct BetaBlockSpec {
    #[confval(label)]
    beta_name: Located<String>,
    value: Located<i64>,
}

#[derive(confval::Spec)]
struct TwoBlockSpec {
    #[confval(nested)]
    alpha: Vec<Located<AlphaBlockSpec>>,
    #[confval(nested)]
    beta: Vec<Located<BetaBlockSpec>>,
}

impl Validate for AlphaBlockSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for BetaBlockSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for TwoBlockSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn scope_labels_reads_the_named_blocks_own_label_field() {
    // Arrange
    // alpha is declared before beta, and each block designates a different label
    // field. Reading beta's labels must use beta_name, not alpha's field.
    let text = "[[beta]]\nbeta_name = \"b1\"\nvalue = 1\n";
    let mut sources = SourceMap::new();
    let id = sources.add("two-block", text);
    let mut report = Report::new();
    let fields = toml::parse_toml_fields(&sources, id, &mut report).expect("the source parses");
    let schema = TwoBlockSpec::schema();

    // Act
    let labels = scope_labels(&fields, &schema, "beta");

    // Assert
    assert_eq!(labels.len(), 1, "beta defines one label");
    assert_eq!(labels[0].value.as_str(), "b1");
}
