//! End-to-end example: parse a JSON config span-first, validate it, lower it to
//! a runtime type, print the result, and emit the populated spec back to
//! canonical JSON text.
//!
//! The spec types, validators, config types, and lowering functions all are in
//! `common`, which the `hcl`, `toml`, `kdl`, and `yaml` examples share
//! verbatim. Those four examples run the same steps in the same order as
//! this one. Only the source text, its file name, and the two format calls,
//! `parse_json` and `emit_json`, differ between the five.
//!
//! A failing variant renders its diagnostics to stderr first, and the valid
//! config then shows the lowered output and the write path. The failing report
//! includes an error at a single list element from a handwritten rule, the same
//! from a recorded keyword set, an unknown keyword in a nested
//! object, and a cross-field warning whose related span points at the setting
//! that caused it.
//!
//! The valid config omits the `limits` member, so the lowered output shows the
//! config-side `#[confval(nested, default)]` filling `LimitsSpec::default()`
//! while the spec stays source-faithful.
//!
//! JSON has one way to nest, the object, which the model reads wherever
//! it accepts a block. The document root must be an object. The frontend
//! accepts strict JSON alone, so a comment or a trailing comma is a syntax
//! error.
//!
//! Run with: cargo run -p confval --example json --features derive,color,json

mod common;

use common::{ServerConfig, ServerSpec, validate_and_gate};
use confval::format::ToFields;
use confval::prelude::*;

/// Parses and validates a broken config, rendering its diagnostics to stderr
/// without stopping the program, so the run shows the report and the valid
/// path in one pass.
fn show_failing_variant() -> Result<(), String> {
    let input = r#"{
  "hostname": "",
  "port": 80,
  "tls": true,
  "allow": ["10.0.0.0/8", ""],
  "log_events": ["request", "shout"],
  "limits": {
    "mode": "yolo"
  }
}
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("broken.json", input);
    let spec: Option<ServerSpec> = confval::format::json::parse_json(&sources, id, &mut report);
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

    let input = r#"{
  "hostname": "127.0.0.1",
  "port": 8443,
  "workers": 8,
  "tls": true,
  "allow": ["10.0.0.0/8", "192.168.0.0/16"],
  "log_events": ["request", "error"],
  "headers": { "Content-Type": "application/json", "X-Env": "prod" }
}
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.json", input);

    // Parse (JSON)
    let spec: Option<ServerSpec> = confval::format::json::parse_json(&sources, id, &mut report);

    let spec = spec.ok_or("parse returned None without reporting an error")?;

    // Validate and gate
    validate_and_gate(&spec, &sources, &mut report);

    // Lower to the runtime config
    let config =
        ServerConfig::lower(&spec, &mut report).ok_or("lowering failed despite a clean report")?;
    println!("{}", config);

    // Emit the populated spec back to canonical JSON, the write path.
    let text =
        confval::format::json::emit_json(&spec.to_fields()).map_err(|error| error.to_string())?;
    println!("+ Emitted JSON:");
    print!("{text}");

    Ok(())
}
