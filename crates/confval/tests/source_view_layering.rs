//! The source view of an assembled spec: a value set only by the environment
//! keeps a real span from its synthetic source, so it appears in the source
//! view exactly as a file-set value does.
//!
//! `env_fields` iterates the process environment, and `set_var` is unsafe in
//! the 2024 edition, so the environment test holds `ENV_LOCK` and no other test
//! in this binary reads the environment.

use confval::format::toml::{emit_toml, parse_toml_fields};
use confval::layering::{Assembly, env_fields};
use confval::prelude::*;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(confval::Spec, Debug)]
struct ServerSpec {
    hostname: Located<String>,
    #[confval(default = 4)]
    workers: Located<i64>,
    port: Option<Located<i64>>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn an_env_only_value_appears_in_the_source_view_of_an_assembled_spec() {
    let _guard = ENV_LOCK.lock().unwrap();

    // Arrange
    // The file sets the hostname, the environment sets the port, and nothing
    // sets workers, so its default fills detached. The source view should show
    // the file value and the environment value, and omit the default.
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let base = sources.add("server.toml", "hostname = \"filehost\"\n");
    // Sound: `ENV_LOCK` is held and no other test reads the environment.
    unsafe {
        std::env::set_var("CONFVAL_SOURCE_VIEW_TEST_PORT", "9090");
    }
    let from_file = parse_toml_fields(&sources, base, &mut report);
    let from_env = env_fields(&mut sources, "CONFVAL_SOURCE_VIEW_TEST_", &mut report);
    unsafe {
        std::env::remove_var("CONFVAL_SOURCE_VIEW_TEST_PORT");
    }

    // Act
    let spec: ServerSpec = Assembly::new()
        .merge(from_file)
        .merge(from_env)
        .assemble(&mut report)
        .expect("assembles");

    // Assert
    let fields = spec.to_source_fields();
    let names: Vec<String> = fields.iter().map(|field| field.name.clone()).collect();
    let toml = emit_toml(&fields).expect("emit toml");
    assert_eq!(names, vec!["hostname", "port"]);
    assert!(toml.contains("hostname = \"filehost\""), "got:\n{toml}");
    assert!(toml.contains("port = 9090"), "got:\n{toml}");
    assert!(!toml.contains("workers"), "got:\n{toml}");
}
