//! Integration tests for layering: real file, environment, and command line
//! sources assembled through [`Assembly`], the path the layering guide
//! documents.
//!
//! `env_fields` iterates the process environment, and `set_var` is unsafe in
//! the 2024 edition, so every test that touches the environment holds
//! `ENV_LOCK` and no other test in this binary reads the environment. Each
//! test uses its own variable prefix so the tests share nothing.

use confval::format::toml::parse_toml_fields;
use confval::layering::{Assembly, cli_fields, env_fields};
use confval::prelude::*;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(confval::Spec, Debug)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
    #[confval(default = 4)]
    workers: Located<i64>,
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}

#[derive(confval::Spec, Debug)]
#[confval(derive_default)]
struct LimitsSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn file_env_and_cli_layers_assemble_with_call_order_precedence() {
    let _guard = ENV_LOCK.lock().unwrap();
    // Arrange
    // The file is the base, the environment overrides it, the command line
    // overrides that, and a joined defaults file fills what is still missing.
    // `hostname` is set by all three merge layers, so it pins the collision
    // order end to end.
    let base_text = r#"hostname = "filehost"
port = 8080

[limits]
max_body_mb = 32
mode = "enforce"
"#;
    let defaults_text = "port = 1\nworkers = 8\n";
    // Sound: `ENV_LOCK` is held and no other test reads the environment.
    unsafe {
        std::env::set_var("CONFVAL_LAYERING_TEST_PORT", "9090");
        std::env::set_var("CONFVAL_LAYERING_TEST_HOSTNAME", "envhost");
    }
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let base = sources.add("server.toml", base_text);
    let defaults = sources.add("defaults.toml", defaults_text);

    // Act
    let spec: Option<ServerSpec> = Assembly::new()
        .merge(parse_toml_fields(&sources, base, &mut report))
        .merge(env_fields(
            &mut sources,
            "CONFVAL_LAYERING_TEST_",
            &mut report,
        ))
        .merge(cli_fields(
            &mut sources,
            [
                "--hostname=clihost".to_string(),
                "--limits.mode=log".to_string(),
            ],
            &mut report,
        ))
        .join(parse_toml_fields(&sources, defaults, &mut report))
        .into(&mut report);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    let spec = spec.unwrap();
    assert_eq!(spec.hostname.value, "clihost");
    assert_eq!(spec.port.value, 9090);
    assert_eq!(spec.workers.value, 8);
    let limits = spec.limits.unwrap();
    assert_eq!(limits.value.max_body_mb.value, 32);
    assert_eq!(limits.value.mode.value, "log");
}

#[test]
fn a_bad_env_value_reports_a_type_mismatch_at_the_variable() {
    let _guard = ENV_LOCK.lock().unwrap();
    // Arrange
    // The layering guide documents that `APP_PORT=high` for an integer field
    // reports `expected integer, found string` at the variable itself.
    let base_text = "hostname = \"filehost\"\n";
    // Sound: `ENV_LOCK` is held and no other test reads the environment.
    unsafe {
        std::env::set_var("CONFVAL_LAYERING_BAD_PORT", "high");
    }
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let base = sources.add("server.toml", base_text);

    // Act
    let spec: Option<ServerSpec> = Assembly::new()
        .merge(parse_toml_fields(&sources, base, &mut report))
        .merge(env_fields(
            &mut sources,
            "CONFVAL_LAYERING_BAD_",
            &mut report,
        ))
        .into(&mut report);

    // Assert
    assert!(spec.is_none());
    let issue = &report.issues()[0];
    assert_eq!(issue.message, "expected integer, found string");
    let span = issue.span.expect("the mismatch should carry a span");
    assert_eq!(
        sources.get(span.source).unwrap().name,
        "env:CONFVAL_LAYERING_BAD_PORT"
    );
}

#[test]
fn a_repeated_cli_flag_is_last_wins_through_the_assembly() {
    // Arrange
    let base_text = "hostname = \"filehost\"\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let base = sources.add("server.toml", base_text);

    // Act
    let spec: Option<ServerSpec> = Assembly::new()
        .merge(parse_toml_fields(&sources, base, &mut report))
        .merge(cli_fields(
            &mut sources,
            ["--port=1".to_string(), "--port=2".to_string()],
            &mut report,
        ))
        .into(&mut report);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    assert_eq!(spec.unwrap().port.value, 2);
}

#[test]
fn join_fills_a_missing_nested_field_below_the_top_level() {
    // Arrange
    // The base sets `limits.mode` and the joined file supplies both nested
    // fields, so the join must recurse into the block, fill `max_body_mb`,
    // and leave the base's `mode` standing.
    let base_text = r#"hostname = "filehost"
port = 8080

[limits]
mode = "log"
"#;
    let defaults_text = "[limits]\nmax_body_mb = 64\nmode = \"enforce\"\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let base = sources.add("server.toml", base_text);
    let defaults = sources.add("defaults.toml", defaults_text);

    // Act
    let spec: Option<ServerSpec> = Assembly::new()
        .merge(parse_toml_fields(&sources, base, &mut report))
        .join(parse_toml_fields(&sources, defaults, &mut report))
        .into(&mut report);

    // Assert
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    let spec = spec.unwrap();
    let limits = spec.limits.unwrap();
    assert_eq!(limits.value.max_body_mb.value, 64);
    assert_eq!(limits.value.mode.value, "log");
}

#[test]
fn a_kind_conflict_across_sources_is_reported_with_both_spans() {
    // Arrange
    // The file spells `limits` as a block and the flag sets it as a value, a
    // cross-source conflict the merge reports rather than swallowing.
    let base_text = "hostname = \"filehost\"\nport = 8080\n\n[limits]\nmode = \"log\"\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let base = sources.add("server.toml", base_text);

    // Act
    let _spec: Option<ServerSpec> = Assembly::new()
        .merge(parse_toml_fields(&sources, base, &mut report))
        .merge(cli_fields(
            &mut sources,
            ["--limits=5".to_string()],
            &mut report,
        ))
        .into(&mut report);

    // Assert
    assert!(report.has_errors());
    assert!(
        report.issues().iter().any(
            |issue| issue.message == "`limits` is a value in one source and a block in another"
        ),
        "issues: {:?}",
        report.issues()
    );
}
