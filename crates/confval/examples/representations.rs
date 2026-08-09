//! Three representations of one loaded spec, each answering a different
//! question about the configuration.
//!
//! The source view shows what the operator actually set, with defaults omitted.
//! The populated view shows what the service resolved to after defaults. The
//! runtime view shows the typed values the program runs on.
//!
//! The source sets only `mode`, leaving `max_body_mb` to its default, so the
//! three views differ exactly where a default fills a gap. The `mode` field is
//! a `keyword_enum!`, and every view shows it as `"log"`, including the runtime
//! view's JSON, because the macro's serde impl writes the keyword rather than
//! the Rust variant name.
//!
//! Run with: cargo run -p confval --example representations --features derive,serde,toml

use confval::format::toml::{emit_toml, parse_toml};
use confval::prelude::*;

keyword_enum!(Mode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

#[derive(confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}

impl Validate for LimitsSpec {
    fn validate(&self, report: &mut Report) {
        Mode::keyword_set().check_located(&self.mode, "mode", report);
    }
}

#[derive(confval::Config, serde::Serialize)]
#[confval(lower_from = LimitsSpec)]
struct LimitsConfig {
    #[confval(lower(from = max_body_mb, with = narrow::i64_to_u16))]
    max_body_mb: u16,
    #[confval(lower(from = mode, with = narrow::keyword::<Mode>))]
    mode: Mode,
}

fn main() -> Result<(), String> {
    let input = "mode = \"log\"\n";

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("limits.toml", input);

    let spec: LimitsSpec = parse_toml(&sources, id, &mut report)
        .ok_or("parse returned None without reporting an error")?;

    // Source view: only what the operator set.
    let source = emit_toml(&spec.to_source_fields()).map_err(|error| error.to_string())?;
    println!("+ Source view (what was set):");
    print!("{source}");

    // Populated view: every default applied.
    let populated = emit_toml(&spec.to_fields()).map_err(|error| error.to_string())?;
    println!();
    println!("+ Populated view (after defaults):");
    print!("{populated}");

    // Runtime view: the typed values the program runs on.
    let config =
        LimitsConfig::lower(&spec, &mut report).ok_or("lowering failed despite a clean report")?;
    let runtime = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    println!();
    println!("+ Runtime view (what runs):");
    println!("{runtime}");

    Ok(())
}
