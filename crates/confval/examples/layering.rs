//! End-to-end example: assemble one configuration from a base file, a defaults
//! file, environment variables, and command line flags, then validate and
//! lower it.
//!
//! The spec types, validators, config types, and lowering functions live in
//! `common`, shared with the `hcl` and `toml` examples. The same `ServerSpec`
//! is populated from four sources instead of one.
//!
//! `merge` applies a source in precedence order, so a later source overrides an
//! earlier one. `join` fills only the fields still missing, without overriding.
//! Environment and command line values arrive as strings and are coerced to
//! each field's declared type, so `APP_PORT=9090` sets the `i64` port and
//! `--tls=true` sets the `bool` tls.
//!
//! Run with: cargo run -p confval --example layering --features derive,color,toml,layering

mod common;

use common::{ServerConfig, ServerSpec, validate_and_gate};
use confval::format::toml::parse_toml_fields;
use confval::layering::{Assembly, cli_fields, env_fields};
use confval::prelude::*;

fn main() -> Result<(), String> {
    let base_text = r#"hostname = "127.0.0.1"
port = 8080

[limits]
max_body_mb = 32
mode = "enforce"
"#;

    // A fallback layer, joined last, that supplies values no other source set.
    let defaults_text = r#"port = 1
workers = 8
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
    let defaults = sources.add("defaults.toml", defaults_text);

    // Each source is a provider that yields the same neutral `Fields`. The
    // `merge` calls set precedence: the file is the base, the environment
    // (prefix `APP_`) overrides it, and the command line overrides that. `join`
    // adds the defaults file last, filling only what is still missing, so it
    // supplies `workers` but does not touch the `port` the environment set.
    // Environment and command line values are strings, and each is coerced to
    // the field's declared type.
    let spec: Option<ServerSpec> = Assembly::new()
        .merge(parse_toml_fields(&sources, base, &mut report))
        .merge(env_fields(&mut sources, "APP_", &mut report))
        .merge(cli_fields(
            &mut sources,
            [
                "--limits.max_body_mb=64".to_string(),
                "--tls=true".to_string(),
            ],
            &mut report,
        ))
        .join(parse_toml_fields(&sources, defaults, &mut report))
        .assemble(&mut report);

    let spec = spec.ok_or("a source produced no tree (see the report)")?;

    validate_and_gate(&spec, &sources, &mut report);
    let config =
        ServerConfig::lower(&spec, &mut report).ok_or("lowering failed despite a clean report")?;

    print!("{}", config);
    println!("tls: {}", config.tls);
    Ok(())
}
