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

### Feature flags

| Flag       | Default | Brings in        | Enables                                                                                  |
|------------|---------|------------------|------------------------------------------------------------------------------------------|
| `serde`    | off     | `serde`          | `Located` serde impls, `render_json`                                                     |
| `color`    | off     | `owo-colors`     | `render_pretty` with ANSI color                                                          |
| `hcl`      | off     | `hcl-edit`       | The `confval::format::hcl` frontend                                                      |
| `toml`     | off     | `toml_edit`      | The `confval::format::toml` frontend                                                     |
| `kdl`      | off     | `kdl`            | The `confval::format::kdl` frontend                                                      |
| `json`     | off     | `jsonc-parser`   | The `confval::format::json` frontend                                                     |
| `yaml`     | off     | `saphyr-parser`  | The `confval::format::yaml` frontend                                                     |
| `derive`   | off     | `confval-derive` | `#[derive(Spec)]` and `#[derive(Config)]` (format-neutral)                               |
| `layering` | off     | nothing          | The `confval::layering` module for assembling from a file, environment, and command line |

Frontends (that define the configuration format) are independent opt-ins.
Pick `hcl`, `toml`, `kdl`, `json`, or `yaml` for the format you want.
The `derive` feature emits the format-neutral `FromFields`, so it brings in no parser on its own.
The `layering` feature adds the [layering](./guide/layering.md) module, which merges several sources into one configuration.
It pulls in no external crate.

## A complete example

This example parses an HCL document, validates it, checks the report for errors, and lowers the validated spec into a runtime config.

The crate ships the same program as multiple runnable examples.
`hcl.rs`, `toml.rs`, `kdl.rs`, `json.rs`, and `yaml.rs` each supply a source document and the two format calls that parse and emit it.
All five pull everything after parsing from a shared `common/mod.rs`.

Read through it once for the overall shape.

```rust
use confval::prelude::*;

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
    port: Located<i64>,
    #[confval(default = 4)]
    workers: Located<i64>,
    #[confval(default = false)]
    tls: Located<bool>,
    // Optional in the source. When the block is omitted, the spec keeps it
    // `None`, so a spec dump stays source-faithful. The config side fills the
    // default at lowering time.
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}

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
        MAX_BODY_MB.check_located(&self.max_body_mb, "max_body_mb", report);
        LimitMode::keyword_set().check_located(&self.mode, "mode", report);
    }
}

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        PORT.check_located(&self.port, "port", report);
        WORKERS.check_located(&self.workers, "workers", report);

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
    }
}

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    hostname: String,
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
    #[confval(lower(from = workers, with = workers_to_usize))]
    workers: usize,
    tls: bool,
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

fn workers_to_usize(value: &Located<i64>, _report: &mut Report) -> Option<usize> {
    // Safe: the range was validated and lowering only runs on a clean report.
    Some(value.value as usize)
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

    if report.has_errors() {
        let mut out = String::new();
        report.render_pretty(&sources, &mut out).unwrap();
        eprint!("{out}");
        std::process::exit(1);
    }

    let spec = spec.expect("parse returned None without reporting an error");
    let config = ServerConfig::lower(&spec, &mut report).expect("validated config lowers");
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
- The `Validate` impls check what the values mean and report at each field's span.
  See [Validation](./guide/validation.md).
- The config types, `ServerConfig` and `LimitsConfig`, are the runtime form the validated spec lowers into.
  See [Lowering](./guide/lowering.md).
- The `main` function runs the stages in order: parse, validate, check `has_errors`, then lower.
  See [Diagnostics](./guide/diagnostics.md) for how the report renders.

To watch the report work, put some bad values in the input: an empty `hostname`, a `port` of `99999`, an unknown `mode`.
The `has_errors` check stops the run before lowering.
All three problems come back reported, each at its own line and column.

## Running the examples

The program above ships as the `hcl`, `toml`, `kdl`, `json`, and `yaml` examples in `crates/confval/examples/`, alongside examples for warnings, validation traversal, layering, and templates.
See [Examples](./examples.md) for each run command and the output it prints.
