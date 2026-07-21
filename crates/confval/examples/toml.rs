//! End-to-end example: parse a TOML config span-first, validate it, lower it
//! to a runtime type, and print the result.
//!
//! This is the companion to the `hcl` example. The spec types, validators,
//! config types, and lowering functions all live in `common`, which both
//! examples share verbatim. Only the source text and the single `parse_toml`
//! call below are format-specific.
//!
//! Where the `hcl` example feeds an invalid config to show the diagnostics,
//! this one feeds a valid config to show the lowered output.
//!
//! The `limits` block is omitted here, so the output shows the config-side
//! `#[confval(nested, default)]` materializing `LimitsSpec::default()` at
//! runtime while the spec stays source-faithful.
//!
//! Run with: cargo run -p confval --example toml --features derive,color,toml

// unwrap/expect are fine in a self-contained example.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use crate::common::evaluate_report;
use common::{ServerConfig, ServerSpec, validate_server};
use confval::prelude::*;

fn main() {
    let input = r#"hostname = "127.0.0.1"
port = 8080
workers = 8
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", input);

    // Parse (toml)
    let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);

    // Validate
    if let Some(spec) = &spec {
        validate_server(spec, &mut report);
    }

    // Gate
    evaluate_report(&sources, &report);
    let spec = spec.expect("parse returned None without reporting an error");

    // Lower
    let config = ServerConfig::lower(&spec, &mut report).expect("validated config lowers");

    // Print results
    println!("{}", config);
}
