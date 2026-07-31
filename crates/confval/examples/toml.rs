//! End-to-end example: parse a TOML config span-first, validate it, lower it
//! to a runtime type, and print the result.
//!
//! This is the companion to the `hcl` and `kdl` examples. The spec types,
//! validators, config types, and lowering functions all live in `common`,
//! which all three share verbatim. Only the source text and the single
//! `parse_toml` call below are format-specific.
//!
//! Where the `hcl` example leads with an invalid config to show the
//! diagnostics, this one feeds a valid config to show the lowered output,
//! including a list field whose elements each carry their own span.
//!
//! The `limits` block is omitted here, so the output shows the config-side
//! `#[confval(nested, default)]` materializing `LimitsSpec::default()` at
//! runtime while the spec stays source-faithful.
//!
//! Run with: cargo run -p confval --example toml --features derive,color,toml

mod common;

use common::{ServerConfig, ServerSpec, validate_and_gate};
use confval::prelude::*;

fn main() -> Result<(), String> {
    let input = r#"hostname = "127.0.0.1"
port = 8080
workers = 8
allow = ["10.0.0.0/8", "192.168.0.0/16"]
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", input);

    // Parse (toml)
    let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);

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
