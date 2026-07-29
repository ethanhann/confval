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
//! the doc comment above each field for the annotated form. The example defines
//! its own types rather than reusing `common`, so the output shows only the
//! write path.
//!
//! Run with: cargo run -p confval --example templates --features derive,color,toml,toml

use confval::format::{FieldKind, Fields, Scalar, Value, ValueKind};
use confval::prelude::*;

#[derive(confval::Spec)]
struct ServerSpec {
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
    /// Request size and mode limits.
    #[confval(nested, default)]
    limits: Option<Located<LimitsSpec>>,

    #[confval(nested, default)]
    widget: Located<WidgetSpec>,
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

#[derive(confval::Spec)]
#[confval(derive_default)]
struct WidgetSpec {
    #[confval(nested, default)]
    sprocket: Located<SprocketSpec>,

    #[confval(default = 16)]
    max_weight: Located<i64>,

    #[confval(nested, default)]
    sprocket2: Located<SprocketSpec>,
}

impl Validate for WidgetSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct SprocketSpec {
    #[confval(default = 32)]
    max_height: Located<i64>,
}

impl Validate for SprocketSpec {
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

    println!("populated field model:");
    print_fields(&fields, 0);

    // Emit the populated model back to TOML text, the write path's second half.
    // Emitting a populated spec to TOML never fails, so the error maps to a
    // message only to satisfy the signature.
    let text = confval::format::toml::emit_toml(&fields).map_err(|error| error.to_string())?;
    println!();
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

    // Now, do the same for HCL - emit the raw template.
    let text = confval::format::hcl::emit_hcl(&fields).map_err(|error| error.to_string())?;
    println!();
    println!("+ Emitted HCL:");
    print!("{text}");

    // Emit an annotated HCL template.
    let template =
        confval::format::hcl::emit_hcl(&spec.to_template()).map_err(|error| error.to_string())?;
    println!();
    println!("+ emitted HCL template with annotations:");
    print!("{template}");

    Ok(())
}

/// Prints a field level as indented `name = value` lines and `name { ... }`
/// blocks, so the filled `limits` block is visible in the output.
fn print_fields(fields: &Fields, depth: usize) {
    let indent = "  ".repeat(depth);
    for field in fields.iter() {
        match &field.kind {
            FieldKind::Block(inner) => {
                println!("{indent}{} {{", field.name);
                print_fields(inner, depth + 1);
                println!("{indent}}}");
            }
            FieldKind::Value(value) => {
                println!("{indent}{} = {}", field.name, render_value(value));
            }
        }
    }
}

fn render_value(value: &Value) -> String {
    match &value.kind {
        ValueKind::Scalar(scalar) => render_scalar(scalar),
        ValueKind::Seq(elements) => {
            let rendered: Vec<String> = elements.iter().map(render_value).collect();
            format!("[{}]", rendered.join(", "))
        }
        ValueKind::Map(_) => "{ ... }".to_string(),
        ValueKind::Other(label) => label.to_string(),
    }
}

fn render_scalar(scalar: &Scalar) -> String {
    match scalar {
        Scalar::String(text) => format!("{text:?}"),
        Scalar::Int(number) => number.to_string(),
        Scalar::Float(number) => number.to_string(),
        Scalar::Bool(flag) => flag.to_string(),
        Scalar::Unparsed(text) => format!("{text:?}"),
    }
}
