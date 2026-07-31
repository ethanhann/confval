//! The ready-made `narrow` helpers: spec integers are `i64`, runtime types use
//! the exact width they need, and the helpers convert between the two.
//!
//! Each helper slots directly into `#[confval(lower(from = ..., with = ...))]`.
//! The config below narrows a port to `u16`, a connection count to `u32`,
//! converts two second counts to `Duration`, and widens a sampling knob to
//! `f64`. The optional variants pass an absent field through as `None`.
//!
//! The helpers are also the checked backstop behind the error gate. Lowering
//! only runs on a clean report, so a value that does not fit its runtime width
//! means a validation rule is missing. This spec deliberately validates
//! nothing, and the failing variant shows a helper reporting the out-of-range
//! value at its span instead of truncating it.
//!
//! Run with: cargo run -p confval --example narrow --features derive,color,toml

use confval::prelude::*;
use std::time::Duration;

#[derive(confval::Spec)]
struct ServiceSpec {
    port: Located<i64>,
    max_connections: Located<i64>,
    shutdown_timeout_secs: Located<i64>,
    sample_per_thousand: Located<i64>,
    request_timeout_secs: Option<Located<i64>>,
}

// Empty on purpose: the failing variant below relies on nothing catching the
// bad port before lowering. A real spec would pair the `u16` field with a
// `range_constraint!` so the operator sees the friendlier message.
impl Validate for ServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Config)]
#[confval(lower_from = ServiceSpec)]
struct ServiceConfig {
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
    #[confval(lower(from = max_connections, with = narrow::i64_to_u32))]
    max_connections: u32,
    #[confval(lower(from = shutdown_timeout_secs, with = narrow::i64_secs_to_duration))]
    shutdown_timeout: Duration,
    #[confval(lower(from = sample_per_thousand, with = narrow::i64_to_f64))]
    sample_per_thousand: f64,
    #[confval(lower(from = request_timeout_secs, with = narrow::opt_i64_secs_to_duration))]
    request_timeout: Option<Duration>,
}

/// Parses one config, lowers it, and prints either the runtime values or the
/// report the narrowing helpers filled.
fn run(label: &str, input: &str) -> Result<(), String> {
    println!("{label}");

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("service.toml", input);

    let spec: Option<ServiceSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);
    let spec = spec.ok_or("parse returned None without reporting an error")?;
    spec.validate_all(&mut report);

    match ServiceConfig::lower(&spec, &mut report) {
        Some(config) => {
            println!(
                "port {} accepting {} connections, shutdown after {:?}",
                config.port, config.max_connections, config.shutdown_timeout
            );
            println!(
                "sampling {} per thousand, request timeout {:?}\n",
                config.sample_per_thousand, config.request_timeout
            );
        }
        None => {
            let mut out = String::new();
            report
                .render_pretty(&sources, &mut out)
                .map_err(|error| error.to_string())?;
            print!("{out}");
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    // `request_timeout_secs` is absent, so the optional helper lowers `None`.
    run(
        "in range: every helper narrows cleanly",
        r#"port = 8080
max_connections = 10000
shutdown_timeout_secs = 30
sample_per_thousand = 250
"#,
    )?;

    run(
        "out of range: the helper reports at the span instead of truncating",
        r#"port = 99999
max_connections = 10000
shutdown_timeout_secs = 30
sample_per_thousand = 250
"#,
    )
}
