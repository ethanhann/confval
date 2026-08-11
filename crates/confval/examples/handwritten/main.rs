//! A spec written by hand, for the shapes `#[derive(Spec)]` cannot express.
//!
//! The derive handles a fixed set of field shapes. A block whose `mode` field
//! decides which fields follow is not one of them. A spec with that shape
//! implements the same traits the derive would have implemented, using the same
//! helpers.
//!
//! The root here is handwritten, its `limits`, `telemetry`, and `route`
//! children are derived, and the `tls` block inside a derived route is
//! handwritten again. Generated code therefore calls handwritten impls, and
//! handwritten code calls generated impls, over one document in one run.
//!
//! A handwritten spec type implements four traits. `FromFields` and `ToFields`
//! are the read and write halves. `Validate` holds the rules.
//! `ValidateNested` is the traversal the derive would have written. The
//! `Self: ValidateNested` bound on `validate_all` makes omitting
//! `ValidateNested` a compile error rather than a silently skipped subtree. A
//! type in a required nested slot needs `Default` as well, because the
//! generated parser fills an absent block with it before reporting the block
//! missing.
//!
//! The test at `crates/confval/tests/handwritten_parity.rs` writes one spec
//! both ways and asserts that the two walks render the same text and agree on
//! every span. This example prints each stage instead of repeating that check.
//!
//! Run with: cargo run -p confval --example handwritten --features derive,color,toml,hcl

mod children;
mod runtime;
mod spec;
mod tls;

use confval::format::hcl::emit_hcl;
use confval::format::toml::{emit_toml, parse_toml};
use confval::prelude::*;
use runtime::ServiceConfig;
use spec::ServiceSpec;

/// The document an operator wrote. `workers`, `sample_rate`, `verbose`, and the
/// whole `telemetry` block are left out. The three write walks therefore
/// differ.
const DOCUMENT: &str = r#"name = "edge"
events = ["started", "stopped"]
headers = { "Content-Type" = "application/json", "cache.max-age" = "60" }

[limits]
max_body_mb = 32

[[route]]
path = "/api"
upstream = "api.internal:8080"

  [route.tls]
  mode = "acme"
  domains = ["a.example.com", "b.example.com"]
  challenge = "http-01"

[[route]]
path = "/static"
upstream = "cdn.internal:80"

  [route.tls]
  mode = "manual"
  cert = "/etc/tls/edge.crt"
  key = "/etc/tls/edge.key"
"#;

/// A document with one problem per kind of type: an unknown field in the
/// handwritten root, a bad value in a derived route, and a missing required
/// field in the handwritten tagged enum.
const BROKEN: &str = r#"name = "edge"
verbosity = true

[limits]
max_body_mb = 32

[[route]]
path = "api"
upstream = "api.internal:8080"

  [route.tls]
  mode = "manual"
  cert = "/etc/tls/edge.crt"
"#;

fn parse(label: &str, input: &str) -> Option<(SourceMap, ServiceSpec)> {
    let mut sources = SourceMap::new();
    let id = sources.add("service.toml", input);
    let mut report = Report::new();

    let spec: Option<ServiceSpec> = parse_toml(&sources, id, &mut report);
    if let Some(spec) = &spec {
        spec.validate_all(&mut report);
    }

    if report.has_issues() {
        let mut out = String::new();
        match report.render_pretty(&sources, &mut out) {
            Ok(()) => print!("{out}"),
            Err(error) => println!("could not render the report: {error}"),
        }
    }
    if report.has_errors() {
        println!("{label}: the report gated lowering\n");
        return None;
    }
    spec.map(|spec| (sources, spec))
}

fn main() -> Result<(), String> {
    println!("+ Diagnostics");
    println!("Three errors from three kinds of type, reported the same way.\n");
    parse("broken", BROKEN);

    let Some((_sources, spec)) = parse("good", DOCUMENT) else {
        return Err("the good document should parse".to_string());
    };

    println!("+ Runtime");
    println!("The Config derive lowers a handwritten root.");
    println!("A derived route lowers its tls field through a handwritten Lower impl.\n");
    let mut report = Report::new();
    let config =
        ServiceConfig::lower(&spec, &mut report).ok_or("lowering failed on a clean report")?;
    println!("{config:#?}\n");

    println!("+ Populated view");
    println!("to_fields over the mixed tree. Every default is filled, whether");
    println!("the node that holds it was generated or written by hand.\n");
    let populated = emit_toml(&spec.to_fields()).map_err(|error| error.to_string())?;
    print!("{populated}");

    println!("\n+ Source view");
    println!("to_source_fields over the same tree. What the operator left out is");
    println!("dropped at every level, including inside the handwritten blocks.\n");
    let source = emit_toml(&spec.to_source_fields()).map_err(|error| error.to_string())?;
    print!("{source}");

    println!("\n+ Template comments");
    println!("to_template defaults to to_fields for a handwritten impl.");
    println!("That fallback recurses with to_fields, so comments stop at the");
    println!("first handwritten node and never reach anything below it.");
    println!("The root here is handwritten, so the whole document comes out");
    println!("bare, including the derived blocks that do carry doc comments.\n");
    let template = emit_toml(&spec.to_template()).map_err(|error| error.to_string())?;
    print!("{template}");

    println!("\nAsk a derived node for its own template and the comments are");
    println!("there, up to the handwritten tls block inside it.\n");
    let route = &spec.routes.first().ok_or("the fixture has a route")?.value;
    let route_template = emit_toml(&route.to_template()).map_err(|error| error.to_string())?;
    print!("{route_template}");

    println!("\n+ Any format");
    println!("A handwritten ToFields feeds the same emitters as a generated one.\n");
    let hcl = emit_hcl(&spec.to_source_fields()).map_err(|error| error.to_string())?;
    print!("{hcl}");

    Ok(())
}
