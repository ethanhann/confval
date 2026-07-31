//! End-to-end example: parse an HCL config span-first, validate it, lower it
//! to a runtime type, and render the diagnostics.
//!
//! The spec types, validators, config types, and lowering functions all live in
//! `common`, which the companion `toml` and `kdl` examples share verbatim. Only
//! the source text and the single `parse_hcl` call below are format-specific,
//! which is what makes the pipeline after parsing format-neutral.
//!
//! A failing variant renders its diagnostics to stderr first, and the valid
//! config then shows the lowered output. The failing report includes an error
//! at a single list element and a cross-field warning whose related span points
//! at the setting that caused it.
//!
//! Beyond the flat fields, the pair exercises a nested block that is optional
//! in the source but defaulted at runtime (`limits`), a `KeywordSet`-validated
//! keyword field (`mode`), and the ready-made `narrow` helpers alongside a
//! handwritten `with` function.
//!
//! Run with: cargo run -p confval --example hcl --features derive,color,hcl

mod common;

use common::{ServerConfig, ServerSpec, validate_and_gate};
use confval::prelude::*;

/// Parses and validates a broken config, rendering its diagnostics to stderr
/// without stopping the program, so the run shows the report and the valid
/// path in one pass.
fn show_failing_variant() -> Result<(), String> {
    let input = r#"hostname = ""
port = 80
tls = true
allow = ["10.0.0.0/8", ""]

limits {
  mode = "yolo"
}
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("broken.hcl", input);
    let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);
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
tls = true
allow = ["10.0.0.0/8", "192.168.0.0/16"]
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.hcl", input);

    // Parse (HCL)
    let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);

    let spec = spec.ok_or("parse returned None without reporting an error")?;

    // Validate and gate
    validate_and_gate(&spec, &sources, &mut report);

    // Lower
    let config =
        ServerConfig::lower(&spec, &mut report).ok_or("lowering failed despite a clean report")?;

    // Print results
    println!("{}", config);
    Ok(())
}
