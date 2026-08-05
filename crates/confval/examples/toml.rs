//! End-to-end example: parse a TOML config span-first, validate it, lower it to
//! a runtime type, print the result, and emit the populated spec back to
//! canonical TOML text.
//!
//! The spec types, validators, config types, and lowering functions all live in
//! `common`, which the `hcl` and `kdl` examples share verbatim. Those two
//! examples run the same steps in the same order as this one. Only the source
//! text, its file name, and the two format calls, `parse_toml` and `emit_toml`,
//! differ between the three.
//!
//! A failing variant renders its diagnostics to stderr first, and the valid
//! config then shows the lowered output and the write path. The failing report
//! includes an error at a single list element, an unknown keyword in a nested
//! block, and a cross-field warning whose related span points at the setting
//! that caused it.
//!
//! The valid config omits the `limits` table, so the lowered output shows the
//! config-side `#[confval(nested, default)]` filling `LimitsSpec::default()`
//! while the spec stays source-faithful.
//!
//! Run with: cargo run -p confval --example toml --features derive,color,toml

mod common;

use common::{ServerConfig, ServerSpec, validate_and_gate};
use confval::format::ToFields;
use confval::prelude::*;

/// Parses and validates a broken config, rendering its diagnostics to stderr
/// without stopping the program, so the run shows the report and the valid
/// path in one pass.
fn show_failing_variant() -> Result<(), String> {
    let input = r#"hostname = ""
port = 80
tls = true
allow = ["10.0.0.0/8", ""]

[limits]
mode = "yolo"
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("broken.toml", input);
    let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);
    if let Some(spec) = &spec {
        spec.validate_all(&mut report);
    }
    let mut out = String::new();
    report
        .render_pretty(&sources, &mut out)
        .map_err(|error| error.to_string())?;
    eprintln!("+ Diagnostics for a failing variant:");
    eprint!("{out}");
    Ok(())
}

fn main() -> Result<(), String> {
    show_failing_variant()?;

    let input = r#"hostname = "127.0.0.1"
port = 8443
workers = 8
tls = true
allow = ["10.0.0.0/8", "192.168.0.0/16"]
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", input);

    // Parse (TOML)
    let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);

    let spec = spec.ok_or("parse returned None without reporting an error")?;

    // Validate and gate
    validate_and_gate(&spec, &sources, &mut report);

    // Lower to the runtime config
    let config =
        ServerConfig::lower(&spec, &mut report).ok_or("lowering failed despite a clean report")?;
    println!("{}", config);

    // Emit the populated spec back to canonical TOML, the write path.
    let text =
        confval::format::toml::emit_toml(&spec.to_fields()).map_err(|error| error.to_string())?;
    println!("+ Emitted TOML:");
    print!("{text}");

    Ok(())
}
