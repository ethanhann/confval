//! End-to-end example: parse an HCL config span-first, validate it, lower it
//! to a runtime type, and render the diagnostics.
//!
//! The spec types, validators, config types, and lowering functions all live in
//! `common`, which the companion `toml` example shares verbatim. Only the
//! source text and the single `parse_hcl` call below are format-specific, which
//! is what makes the pipeline after parsing format-neutral.
//!
//! This example feeds an invalid config to show the report. The `toml` example
//! feeds a valid one to show the lowered output.
//!
//! Beyond the flat fields, the pair exercises a nested block that is optional
//! in the source but defaulted at runtime (`limits`), a `KeywordSet`-validated
//! keyword field (`mode`), and the ready-made `narrow` helpers alongside a
//! handwritten `with` function.
//!
//! Run with: cargo run -p confval --example hcl --features derive,color,hcl

mod common;

use common::{ServerConfig, ServerSpec, evaluate_report};
use confval::prelude::*;

fn main() -> Result<(), String> {
    let input = r#"hostname = ""
port = 99999

limits {
  mode = "yolo"
}
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.hcl", input);

    // Parse (HCL)
    let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);

    // Validate
    let spec = spec.ok_or("parse returned None without reporting an error")?;
    spec.validate(&mut report);

    // By design, this HCL example does not make it past this gate.
    evaluate_report(&sources, &report);

    // Lower never runs
    let config = ServerConfig::lower(&spec, &mut report).ok_or("validated config lowers")?;

    // Print results
    println!("{}", config);
    Ok(())
}
