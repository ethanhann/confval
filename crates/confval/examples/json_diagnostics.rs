//! Machine-readable diagnostics: the same report the pretty renderer prints,
//! serialized as JSON for CI and tooling.
//!
//! `render_json` resolves each span to its source name, line, and column, and
//! keeps the raw byte offsets alongside them. A tool consuming the output can
//! jump to the exact location without reparsing the config. The method lives
//! behind the `serde` feature.
//!
//! Run with: cargo run -p confval --example json_diagnostics --features derive,serde,toml

use confval::prelude::*;

range_constraint!(PORT, i64, min: 1, max: 65535);

#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
}

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        PORT.check_located(&self.port, "port", report);

        if self.hostname.value.is_empty() {
            report
                .error("hostname must not be empty")
                .at(self.hostname.span)
                .help("Set hostname to a reachable address, e.g. \"127.0.0.1\".")
                .emit();
        }
    }
}

fn main() -> Result<(), String> {
    let input = r#"hostname = ""
port = 99999
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", input);

    let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);
    let spec = spec.ok_or("parse returned None without reporting an error")?;

    spec.validate_all(&mut report);

    let mut out = String::new();
    report
        .render_json(&sources, &mut out)
        .map_err(|error| error.to_string())?;
    println!("{out}");
    Ok(())
}
