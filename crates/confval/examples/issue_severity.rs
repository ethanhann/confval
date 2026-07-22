//! A warning is reported and the pipeline keeps going. An error stops it.
//!
//! This runs the same TOML pipeline as the `toml` example over the same
//! `common` spec, changing only the hostname. `0.0.0.0` is a legal address,
//! so the rule for it emits a warning rather than an error. The gate blocks
//! on `has_errors`. The warning renders and lowering still runs.
//!
//! Swapping the gate to block on `has_issues` is what makes a warning fatal.
//! That policy belongs to the caller. The gate therefore lives in `common`
//! rather than in confval.
//!
//! Run with: cargo run -p confval --example issue_severity --features derive,color,toml

mod common;

use crate::common::validate_and_gate;
use common::{ServerConfig, ServerSpec};
use confval::prelude::*;

fn main() -> Result<(), String> {
    // Possibly invalid hostname will appear as a warning.
    let input = r#"hostname = "0.0.0.0"
port = 8080
workers = 8
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
    let config = ServerConfig::lower(&spec, &mut report).ok_or("validated config lowers")?;

    // Print results
    println!("{}", config);
    Ok(())
}
