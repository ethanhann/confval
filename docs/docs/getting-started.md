---
sidebar_position: 2
---

# Getting Started

## Installation

Add confval to your `Cargo.toml` and enable the features you need.
The core has no default features, so pick the format frontends and extras you actually use.

```toml
[dependencies]
confval = { version = "0.3", features = ["derive", "hcl", "color"] }
```

See [Feature Flags](#feature-flags) for the full list.

## A complete example

The example below parses an HCL document, validates it, gates on errors, and lowers the validated spec into a runtime config.
The companion TOML example defines the same `ServerSpec` and `ServerConfig` and differs only in the source text and the single parse call, which shows that everything after parsing is format-neutral.

```rust
use confval::prelude::*;

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);

const LIMIT_MODES: [&str; 3] = ["enforce", "log", "off"];

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

fn validate_server(spec: &ServerSpec, report: &mut Report) {
    PORT.check_located(&spec.port, "port", report);
    WORKERS.check_located(&spec.workers, "workers", report);

    if let Some(limits) = &spec.limits {
        MAX_BODY_MB.check_located(&limits.value.max_body_mb, "max_body_mb", report);
        KeywordSet::new(&LIMIT_MODES).check_located(&limits.value.mode, "mode", report);
    }

    if spec.hostname.value.is_empty() {
        report
            .error("hostname must not be empty")
            .at(spec.hostname.span)
            .help("Set hostname to a reachable address, e.g. \"127.0.0.1\".")
            .emit();
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
    mode: String,
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
        validate_server(spec, &mut report);
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

Feed it a document with bad values (an empty `hostname`, a `port` of `99999`, an unknown `mode`) and every problem renders at once, each pointing at its own span.

## Running the crate examples

The crate ships two runnable examples that define the same types and differ only in the format they read.

Run the HCL example:

```shell
cargo run -p confval --example hcl --features derive,color,hcl
```

Run the TOML example:

```shell
cargo run -p confval --example toml --features derive,color,toml
```

## Feature flags

| Flag     | Default | Brings in        | Enables                                                    |
|----------|---------|------------------|------------------------------------------------------------|
| `serde`  | off     | `serde`          | `Located` serde impls, `render_json`                       |
| `color`  | off     | `owo-colors`     | `render_pretty` with ANSI color                            |
| `hcl`    | off     | `hcl-edit`       | The `confval::format::hcl` frontend                        |
| `toml`   | off     | `toml_edit`      | The `confval::format::toml` frontend                       |
| `derive` | off     | `confval-derive` | `#[derive(Spec)]` and `#[derive(Config)]` (format-neutral) |

Frontends are independent opt-ins.
The derive emits the format-neutral `FromFields`, so `derive` brings in no parser on its own.
Pick `hcl` and/or `toml` for the format you actually read.
