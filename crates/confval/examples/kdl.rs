//! End-to-end example: parse a KDL config span-first, validate it, lower it to
//! a runtime type, print the result, and emit the populated spec back to
//! canonical KDL text.
//!
//! The spec types, validators, config types, and lowering functions all live in
//! `common`, which the `hcl`, `toml`, and `json` examples share verbatim. Those
//! three examples run the same steps in the same order as this one. Only the
//! source text, its file name, and the two format calls, `parse_kdl` and
//! `emit_kdl`, differ between the four.
//!
//! A failing variant renders its diagnostics to stderr first, and the valid
//! config then shows the lowered output and the write path. The failing report
//! includes an error at a single list element, an unknown keyword in a nested
//! block, and a cross-field warning whose related span points at the setting
//! that caused it.
//!
//! The valid config omits the `limits` node, so the lowered output shows the
//! config-side `#[confval(nested, default)]` filling `LimitsSpec::default()`
//! while the spec stays source-faithful.
//!
//! KDL writes the same field model differently from HCL and TOML. A scalar is
//! one argument, a list is repeated arguments or repeated nodes, and a nested
//! structure is a children block or properties on one node.
//!
//! Run with: cargo run -p confval --example kdl --features derive,color,kdl

mod common;

use common::{ServerConfig, ServerSpec, validate_and_gate};
use confval::format::ToFields;
use confval::prelude::*;

/// Parses and validates a broken config, rendering its diagnostics to stderr
/// without stopping the program, so the run shows the report and the valid
/// path in one pass.
fn show_failing_variant() -> Result<(), String> {
    let input = r#"hostname ""
port 80
tls #true
allow "10.0.0.0/8" ""

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
port 8443
workers 8
tls #true
allow "10.0.0.0/8" "192.168.0.0/16"
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
    let config =
        ServerConfig::lower(&spec, &mut report).ok_or("lowering failed despite a clean report")?;
    println!("{}", config);

    // Emit the populated spec back to canonical KDL, the write path.
    let text =
        confval::format::kdl::emit_kdl(&spec.to_fields()).map_err(|error| error.to_string())?;
    println!("+ Emitted KDL:");
    print!("{text}");

    Ok(())
}
