//! The write path: parse a minimal config, populate it, and emit it back to
//! text, both plain and as an annotated template.
//!
//! The other examples read a config and run it through the pipeline. This one
//! runs the parse backward. The source sets only `hostname` and `port`, so
//! `to_fields` fills every default the source omitted, the `workers` and `tls`
//! leaves and the whole `limits` block from `LimitsSpec::default()`. Every value
//! it produces carries a detached span, because the data comes from the spec
//! rather than the file.
//!
//! `limits` is an optional block, so parsing leaves it `None` and a spec dump
//! stays source-faithful. The `#[confval(nested, default)]` marker is the
//! populate signal that fills it here.
//!
//! `emit_toml` serializes the populated model to text, and `to_template` adds
//! the doc comment above each field for the annotated form. The unset optional
//! `pid_file` stays out of the plain dump and renders in the template as a
//! commented-out entry, `#pid_file = ""`, with its doc above it. The example
//! defines its own types rather than reusing `common`, so the output shows
//! only the write path. The `doc_fallback` example covers where a block's
//! template comment comes from when the field has no doc of its own.
//!
//! Run with: cargo run -p confval --example templates --features derive,color,toml,hcl

use confval::prelude::*;

#[derive(confval::Spec)]
struct ServerSpec {
    // The attribute form and a `///` comment are equivalent. Every other field
    // here uses the `///` form.
    #[confval(doc = "The address the server binds to.")]
    hostname: Located<String>,
    /// The port the server listens on.
    port: Located<i64>,
    /// The number of worker threads.
    #[confval(default = 4)]
    workers: Located<i64>,
    /// Whether TLS is enabled.
    #[confval(default = false)]
    tls: Located<bool>,
    /// The PID file path. Left unset here, so the template renders it as a
    /// commented-out entry rather than hiding it.
    pid_file: Option<Located<String>>,
    /// Request size and mode limits.
    #[confval(nested, default)]
    limits: Option<Located<LimitsSpec>>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    /// The maximum request body size, in megabytes.
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    /// How limit violations are handled.
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn main() -> Result<(), String> {
    let input = "hostname = \"127.0.0.1\"\nport = 8080\n";

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", input);

    let spec: ServerSpec = confval::format::toml::parse_toml(&sources, id, &mut report)
        .ok_or("parse returned None without reporting an error")?;

    let fields = spec.to_fields();

    // Emit the populated model back to TOML text, the write path's second half.
    // Emitting a populated spec to TOML never fails, so the error maps to a
    // message only to satisfy the signature.
    let text = confval::format::toml::emit_toml(&fields).map_err(|error| error.to_string())?;
    println!("+ Emitted TOML:");
    print!("{text}");

    // Emit an annotated TOML template.
    // This is the same configuration as above, with each field's doc
    // comment rendered above it, harvested from the spec's `///` comments.
    let template =
        confval::format::toml::emit_toml(&spec.to_template()).map_err(|error| error.to_string())?;
    println!();
    println!("+ Emitted TOML template with annotations:");
    print!("{template}");

    // Emit the plain HCL from the same populated model.
    let text = confval::format::hcl::emit_hcl(&fields).map_err(|error| error.to_string())?;
    println!();
    println!("+ Emitted HCL:");
    print!("{text}");

    // Emit an annotated HCL template.
    let template =
        confval::format::hcl::emit_hcl(&spec.to_template()).map_err(|error| error.to_string())?;
    println!();
    println!("+ Emitted HCL template with annotations:");
    print!("{template}");

    Ok(())
}
