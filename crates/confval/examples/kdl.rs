//! End-to-end example: parse a KDL config span-first, validate it, lower it to
//! a runtime type, print the result, and emit the populated spec back to
//! canonical KDL text.
//!
//! The spec types, validators, config types, and lowering functions all live
//! in `common`, which the `hcl` and `toml` examples share verbatim. Only the
//! source text and the two format calls below, `parse_kdl` and `emit_kdl`, are
//! format-specific.
//!
//! A failing variant renders its diagnostics to stderr first, and the valid
//! config then shows the lowered output and the write path. A scalar is one
//! argument, a list is repeated arguments or repeated nodes, and a nested
//! structure is a children block or properties on one node.
//!
//! Run with: cargo run -p confval --example kdl --features derive,color,kdl

mod common;

use crate::common::validate_and_gate;
use common::{ServerConfig, ServerSpec};
use confval::format::ToFields;
use confval::prelude::*;

/// Parses and validates a broken config, rendering its diagnostics to stderr
/// without stopping the program, so the run shows the report and the valid
/// path in one pass.
fn show_failing_variant() -> Result<(), String> {
    let input = r#"hostname ""
port 99999

limits {
  mode "yolo"
}
"#;
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("broken.kdl", input);
    let spec: Option<ServerSpec> = confval::format::kdl::parse_kdl(&sources, id, &mut report);
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

    let input = r#"hostname "127.0.0.1"
port 8080
workers 8

limits {
  mode "log"
}
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.kdl", input);

    // Parse (KDL)
    let spec: Option<ServerSpec> = confval::format::kdl::parse_kdl(&sources, id, &mut report);

    let spec = spec.ok_or("parse returned None without reporting an error")?;

    // Validate and gate
    validate_and_gate(&spec, &sources, &mut report);

    // Lower to the runtime config
    let config = ServerConfig::lower(&spec, &mut report).ok_or("validated config lowers")?;
    println!("{}", config);

    // Emit the populated spec back to canonical KDL, the write path.
    let text =
        confval::format::kdl::emit_kdl(&spec.to_fields()).map_err(|error| error.to_string())?;
    println!("+ Emitted KDL:");
    print!("{text}");

    Ok(())
}
