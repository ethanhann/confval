---
sidebar_position: 1
---

# Getting Started

confval is a Rust crate for parsing, validating, and lowering configuration files.
It records a source span for every parsed value, so a validation error can report the line and column in the file the value came from.

Use it to build the configuration layer of an application.
You define the shape of the config as Rust types, parse a file into those types, run validation, and lower the result into the runtime types the rest of the program uses.

## Installation

Add confval to your `Cargo.toml`.

The crate has no default features.
Enable the format frontends and extras you use.

For example, for TOML format, derive macros, JSON diagnostics, and console color support:

```shell
cargo add confval --features "toml,derive,serde,color"
```

Or, the HCL format, derive macros, and plain output:

```shell
cargo add confval --features "hcl,derive"
```

:::info
confval also ships two agent skills.
One scaffolds a pipeline in a project that has none, and the other keeps the layers in sync when you add a setting.
Install the binary with `cargo install confval`, then run `confval init` to write the skills into your project.
See [Agent Skills](./agent-skills.md) for the full workflow.
:::

### Feature flags

| Flag       | Default | Brings in           | Enables                                                                                  |
|------------|---------|---------------------|------------------------------------------------------------------------------------------|
| `serde`    | off     | `serde`             | `Located` serde impls, `render_json`                                                     |
| `color`    | off     | `annotate-snippets` | `render_pretty` with ANSI color                                                          |
| `hcl`      | off     | `hcl-edit`          | The `confval::format::hcl` frontend                                                      |
| `toml`     | off     | `toml_edit`         | The `confval::format::toml` frontend                                                     |
| `kdl`      | off     | `kdl`               | The `confval::format::kdl` frontend                                                      |
| `json`     | off     | `jsonc-parser`      | The `confval::format::json` frontend                                                     |
| `yaml`     | off     | `saphyr-parser`     | The `confval::format::yaml` frontend                                                     |
| `derive`   | off     | `confval-derive`    | `#[derive(Spec)]` and `#[derive(Config)]` (format-neutral)                               |
| `layering` | off     | nothing             | The `confval::layering` module for assembling from a file, environment, and command line |

Frontends (that define the configuration format) are independent opt-ins.
Pick `hcl`, `toml`, `kdl`, `json`, or `yaml` for the format you want.
The `derive` feature emits the format-neutral `FromFields`, so it brings in no parser on its own.
The `layering` feature adds the [layering](./guide/layering.md) module, which merges several sources into one configuration.
It pulls in no external crate.

## A complete example

This example parses an HCL document, validates it, checks the report for errors, and lowers the validated spec into a runtime config.

The crate ships the same program as multiple runnable examples.
`hcl.rs`, `toml.rs`, `kdl.rs`, `json.rs`, and `yaml.rs` each supply a source document and the two format calls that parse and emit it.
All five pull everything after parsing from a shared `common` module.
The listing below is a trimmed version of that module and the `hcl` example's `main`.

Read through it once for the overall shape.

```rust
use confval::prelude::*;
use std::collections::{BTreeMap, HashMap};

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    #[confval(range = PORT)]
    port: Located<i64>,
    #[confval(default = 4, range = WORKERS)]
    workers: Located<i64>,
    #[confval(default = false)]
    tls: Located<bool>,
    // A list field. The bare `default` reads an absent list as empty. Each
    // element keeps its own span, so a bad entry is reported at that entry.
    #[confval(default)]
    allow: Vec<Located<String>>,
    // An open-ended, string-keyed map, for keys that are not known ahead of
    // time, such as header names.
    #[confval(map, default)]
    headers: BTreeMap<String, Located<String>>,
    // Optional in the source. When the block is omitted, the spec keeps it
    // `None`, so a spec dump stays source-faithful. The config side fills the
    // default at lowering time.
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    #[confval(default = 16, range = MAX_BODY_MB)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string(), keywords = LimitMode)]
    mode: Located<String>,
}

impl Validate for LimitsSpec {
    // `max_body_mb` and `mode` record their constraints with `#[confval(range)]`
    // and `#[confval(keywords)]`, so the derive checks them. Every rule this
    // block has is recorded, so its `Validate` body is empty.
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        // `port` and `workers` record their ranges, so the derive checks them.
        // This body holds only the rules an attribute cannot express.
        if self.hostname.value.is_empty() {
            report
                .error("hostname must not be empty")
                .at(self.hostname.span)
                .help("Set hostname to a reachable address, e.g. \"127.0.0.1\".")
                .emit();
        }

        if self.hostname.value == "0.0.0.0" {
            report
                .warning("hostname set to listen on every available network device")
                .at(self.hostname.span)
                .help("This might be undesired.")
                .emit();
        }

        for entry in &self.allow {
            if entry.value.is_empty() {
                report
                    .error("allow entries must not be empty")
                    .at(entry.span)
                    .help("Remove the entry or set it to a network, e.g. \"10.0.0.0/8\".")
                    .emit();
            }
        }
    }
}

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    hostname: String,
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
    #[confval(lower(from = workers, with = narrow::i64_to_usize))]
    workers: usize,
    tls: bool,
    #[confval(lower(from = allow, with = allow_to_vec))]
    allow: Vec<String>,
    // Auto-mapped from the spec's `BTreeMap<String, Located<String>>`. The
    // `LowerAuto` impl drops each value's span and hands back a plain runtime
    // map.
    headers: HashMap<String, String>,
    // The spec field is `Option<Located<LimitsSpec>>`. With `default` an absent
    // block lowers `LimitsSpec::default()` instead of producing a missing-field
    // error, and the runtime field stays non-optional.
    #[confval(nested, default)]
    limits: LimitsConfig,
}

#[derive(confval::Config)]
#[confval(lower_from = LimitsSpec)]
struct LimitsConfig {
    #[confval(lower(from = max_body_mb, with = narrow::i64_to_u16))]
    max_body_mb: u16,
    #[confval(lower(from = mode, with = narrow::keyword::<LimitMode>))]
    mode: LimitMode,
}

fn allow_to_vec(value: &[Located<String>], _report: &mut Report) -> Option<Vec<String>> {
    Some(value.iter().map(|entry| entry.value.clone()).collect())
}

fn main() {
    let input = r#"hostname = "127.0.0.1"
port = 8080

limits {
  mode = "log"
}
"#;

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.hcl", input);

    let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);
    if let Some(spec) = &spec {
        spec.validate_all(&mut report);
    }

    // Validation ran, so lower only when the spec parsed and the report is
    // clean. A syntax error left `spec` as None, and validation may have added
    // errors.
    let config = if report.has_errors() {
        None
    } else {
        spec.as_ref()
            .and_then(|spec| ServerConfig::lower(spec, &mut report))
    };

    let Some(config) = config else {
        // Render every problem the report collected, then stop. A bad
        // configuration file is reported, never a panic.
        let mut out = String::new();
        let _ = report.render_pretty(&sources, &mut out);
        eprint!("{out}");
        std::process::exit(1);
    };

    println!(
        "listening on {}:{} with {} workers",
        config.hostname, config.port, config.workers
    );
    println!(
        "limits: max_body_mb={} mode={}",
        config.limits.max_body_mb, config.limits.mode
    );
    println!("tls: {}", config.tls);
}
```

## How the example fits together

The program above has four parts.
Each maps to one stage of the [pipeline](pipeline.md) and has its own guide page for the detail.

- The spec types, `ServerSpec` and `LimitsSpec`, declare the fields you parse a file into.
  `#[confval(derive_default)]` on `LimitsSpec` derives its `Default` from the same attribute defaults that fill an omitted field.
  See [Parsing](./guide/parsing.md).
- A mechanical constraint is recorded on its field with `#[confval(range = ...)]` or `#[confval(keywords = ...)]`, and the derive checks it during validation.
  The `Validate` impls hold the remaining rules and report at each field's span.
  See [Validation](./guide/validation.md).
- The config types, `ServerConfig` and `LimitsConfig`, are the runtime form the validated spec lowers into.
  See [Lowering](./guide/lowering.md).
- The `main` function runs the stages in order: parse, validate, check `has_errors`, then lower.
  It handles the parse and lower `Option` values rather than unwrapping them, so a bad file is reported and the program exits rather than panicking.
  See [Diagnostics](./guide/diagnostics.md) for how the report renders.

To watch the report work, put some bad values in the input: an empty `hostname`, a `port` of `99999`, an unknown `mode`.
The `has_errors` check stops the run before lowering.
All three problems come back reported, each at its own line and column.

## Running the examples

The program above ships as the `hcl`, `toml`, `kdl`, `json`, and `yaml` examples in `crates/confval/examples/`, alongside examples for warnings, validation traversal, layering, and templates.
See [Examples](./examples.md) for each run command and the output it prints.
