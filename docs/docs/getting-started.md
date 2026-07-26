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
| `derive`   | off     | `confval-derive` | `#[derive(Spec)]` and `#[derive(Config)]` (format-neutral)                               |
| `layering` | off     | nothing          | The `confval::layering` module for assembling from a file, environment, and command line |

Frontends (that define the configuration format) are independent opt-ins.
Pick `hcl` or `toml` for the format you want.
The `derive` feature emits the format-neutral `FromFields`, so it brings in no parser on its own.
The `layering` feature adds the [layering](./guide/layering.md) module, which merges several sources into one configuration.
It pulls in no external crate.

## A complete example

This example parses an HCL document, validates it, checks the report for errors, and lowers the validated spec into a runtime config.

The crate ships the same program as multiple runnable examples.
`hcl.rs` and `toml.rs` each supply a source document and one parse call, and both pull everything after parsing from a shared `common/mod.rs`.

Read through it once for the overall shape.
The section after it maps each part to the guide page that covers it in depth.

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
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}

#[derive(confval::Spec)]
struct LimitsSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}

impl Default for LimitsSpec {
    fn default() -> Self {
        Self {
            max_body_mb: Located::detached(16),
            mode: Located::detached("enforce".to_string()),
        }
    }
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
}
```

## How the example fits together

The program above has four parts.
Each maps to one stage of the [pipeline](pipeline.md) and has its own guide page for the detail.

- The spec types, `ServerSpec` and `LimitsSpec`, declare the fields you parse a file into. See [Parsing](./guide/parsing.md).
- The `Validate` impls check what the values mean and report at each field's span. See [Validation](./guide/validation.md).
- The config types, `ServerConfig` and `LimitsConfig`, are the runtime form the validated spec lowers into. See [Lowering](./guide/lowering.md).
- The `main` function runs the stages in order: parse, validate, check `has_errors`, then lower. How the report renders is covered in [Diagnostics](./guide/diagnostics.md).

To watch the report work, put some bad values in the input: an empty `hostname`, a `port` of `99999`, an unknown `mode`.
The `has_errors` check stops the run before lowering, and all three come back reported, each at its own line and column.

## Running the crate examples

The crate ships two runnable examples that define the same types and differ only in the format they read.

Run the HCL example:

```shell
cargo run -q -p confval --example hcl --features derive,color,hcl
```

Configuration file is intentionally invalid:

```shell
error: port must be at most 65535
 --> server.hcl:2:8
  |
2 | port = 99999
  |        ^^^^^
  = help: Set port to at most 65535

error: hostname must not be empty
 --> server.hcl:1:12
  |
1 | hostname = ""
  |            ^^
  = help: Set hostname to a reachable address, e.g. "127.0.0.1".

error: unknown mode: yolo
 --> server.hcl:5:10
  |
5 |   mode = "yolo"
  |          ^^^^^^
  = help: expected one of: enforce, log, off
```

Run the TOML example:

```shell
cargo run -q -p confval --example toml --features derive,color,toml
```

Configuration file is validated:

```shell
listening on 127.0.0.1:8080 with 8 workers
limits: max_body_mb=16 mode=enforce
```

Run the issue_severity example that shows a warning:

```shell
cargo run -q -p confval --example issue_severity --features derive,color,toml
```

Configuration file is validated with a warning:

```shell
warning: hostname set to listen on every available network device
 --> server.toml:1:12
  |
1 | hostname = "0.0.0.0"
  |            ^^^^^^^^^
  = help: This might be undesired.

listening on 0.0.0.0:8080 with 8 workers
limits: max_body_mb=16 mode=enforce
```

Run the validate_traversal example that shows what `validate_all` reaches:

```shell
cargo run -q -p confval --example validate_traversal --features derive,color,toml
```

The same invalid nested block is validated twice, differing only in whether the enclosing block is enabled:

```shell
upstream enabled: the nested child is validated
error: attempts must be at most 10
 --> service.toml:5:12
  |
5 | attempts = 99
  |            ^^
  = help: Set attempts to at most 10

upstream disabled: descend breaks, so the child is skipped
no issues
```

Run the layering example that assembles one config from a base file, a joined defaults file, the environment, and the command line:

```shell
cargo run -q -p confval --example layering --features derive,color,toml,layering
```

The environment sets `port` and the nested `limits.mode`, the command line sets `limits.max_body_mb` and `tls`, and the joined defaults file fills `workers`:

```shell
listening on 127.0.0.1:9090 with 8 workers
limits: max_body_mb=64 mode=log
tls: true
```

See [Layering](./guide/layering.md) for how the sources merge and how environment and command line values are coerced.

## Additional Examples

Additional examples are available for reference:

- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1).
- Snakeway reverse proxy's [snakeway-conf crate](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src) (advanced usage)
