//! End-to-end example: assemble one configuration from a file, environment
//! variables, and command line flags, then validate and lower it.
//!
//! The spec types, validators, config types, and lowering functions live in
//! `common`, shared verbatim with the `hcl` and `toml` examples. This example
//! adds nothing to them. The same `ServerSpec` is populated from three sources
//! instead of one.
//!
//! Precedence is the call order. The file is the base, the environment
//! overrides it, and the command line overrides the environment. Environment
//! and command line values arrive as strings and are coerced to each field's
//! declared type, so `APP_PORT=9090` reaches the `i64` port field as a number.
//!
//! Run with: cargo run -p confval --example layering --features derive,color,toml,layering

mod common;

use common::{ServerConfig, ServerSpec, validate_and_gate};
use confval::layering::{Assembly, cli_fields, env_fields};
use confval::prelude::*;

fn main() -> Result<(), String> {
    let base_text = r#"hostname = "127.0.0.1"
port = 8080
workers = 4

[limits]
max_body_mb = 32
mode = "enforce"
"#;

    // The example sets these so its output is deterministic. A real program
    // reads an environment the operator already populated. `set_var` is unsafe
    // in the 2024 edition and is sound here because it runs before any thread
    // is spawned.
    unsafe {
        std::env::set_var("APP_PORT", "9090");
        std::env::set_var("APP_LIMITS__MODE", "log");
    }

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let base = sources.add("server.toml", base_text);

    // Each configuration source is a provider that yields the same neutral
    // `Fields`. The `merge` calls set precedence: the file is the base, the
    // environment (prefix `APP_`) overrides it, and the command line overrides
    // that. Environment and command line values are strings. Each is coerced to
    // the field's declared type, so `APP_PORT=9090` sets the numeric `port`.
    let file_layer = confval::format::toml::parse_toml_fields(&sources, base, &mut report);
    let env_layer = env_fields(&mut sources, "APP_", &mut report);
    let cli_layer = cli_fields(&mut sources, ["--workers=8".to_string()], &mut report);

    let spec: Option<ServerSpec> = Assembly::new()
        .merge(file_layer)
        .merge(env_layer)
        .merge(cli_layer)
        .into(&mut report);

    let spec = spec.ok_or("a source produced no tree (see the report)")?;

    validate_and_gate(&spec, &sources, &mut report);
    let config = ServerConfig::lower(&spec, &mut report).ok_or("validated config lowers")?;

    println!("{}", config);
    Ok(())
}
