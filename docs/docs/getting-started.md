---
sidebar_position: 2
---

# Getting Started

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

| Flag     | Default | Brings in        | Enables                                                    |
|----------|---------|------------------|------------------------------------------------------------|
| `serde`  | off     | `serde`          | `Located` serde impls, `render_json`                       |
| `color`  | off     | `owo-colors`     | `render_pretty` with ANSI color                            |
| `hcl`    | off     | `hcl-edit`       | The `confval::format::hcl` frontend                        |
| `toml`   | off     | `toml_edit`      | The `confval::format::toml` frontend                       |
| `derive` | off     | `confval-derive` | `#[derive(Spec)]` and `#[derive(Config)]` (format-neutral) |

Frontends (that define the configuration format) are independent opt-ins.
Pick `hcl` or `toml` for the format you actually want.
The `derive` emits the format-neutral `FromFields`, so `derive` brings in no parser on its own.

## A complete example

This example parses an HCL document, validates it, checks the report for errors, and lowers the validated spec into a runtime config.
The `toml` example in the crate defines the same `ServerSpec` and `ServerConfig`.
It differs only in the source text and the one parse call, which shows that the code after parsing does not depend on the format.

Read through it once for the overall shape.
The walkthrough below takes it apart one piece at a time, following the [pipeline contract](./guide/pipeline.md) in order.

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

## Walking through the example

The code breaks into four parts: the spec types you parse into, the validation rules, the config types you lower to, and the `main` that ties them together.
Each part lines up with a phase of the [pipeline contract](./guide/pipeline.md).

### The spec types

When a file is parsed, it is deserialized into a spec type.
Every field is wrapped in a [`Located<T>`](./guide/parsing.md#located-values), which links the value back to where it came from in the configuration file.
That location is called a `span`.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
    #[confval(default = 4)]
    workers: Located<i64>,
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}
```

`#[derive(confval::Spec)]` writes the parser for the struct.
It only checks structure: whether each field is present and has the right type.
Anything else is reported.

- `hostname` and `port` are required.
  Leave one out and it comes back as an error.
- `#[confval(default = 4)]` gives `workers` a value of `4` when the file omits it, so it is not reported as missing.
- `#[confval(nested)]` parses `limits` with its own `LimitsSpec` parser, and `Option` makes the whole block optional.

Spec fields use the loosest type that still parses, like `i64` for `port`.
A `port` of `99999` parses fine here.
Validation catches it later, and points at its span.

`LimitsSpec` works the same way.
It also carries a `Default`, which the config side falls back to when the block is missing.

```rust
#[derive(confval::Spec)]
struct LimitsSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}
```

See [Parsing](./guide/parsing.md#defining-a-spec) for the full set of field rules.

### The validation rules

After parsing, validation checks what the values mean: ranges, allowed keywords, and rules that cross more than one field.
Each field already carries its span, so every message it reports points back at the file.

```rust
range_constraint!(PORT, i64, min: 1, max: 65535);

const LIMIT_MODES: [&str; 3] = ["enforce", "log", "off"];

fn validate_server(spec: &ServerSpec, report: &mut Report) {
    PORT.check_located(&spec.port, "port", report);

    if let Some(limits) = &spec.limits {
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
```

- `range_constraint!` declares a numeric bound once.
  `PORT.check_located(&spec.port, "port", report)` flags `port` at its span when it falls outside the range.
- [`KeywordSet`](./guide/validation.md#keywordset) checks a value against a fixed set of allowed strings.
- For anything else, go through the report builder: `.at(span)` attaches the location, `.help(text)` adds a suggestion, and `.emit()` records the issue.

Validation never stops at the first problem.
It adds every issue it finds to the report, so one run shows you all of them.

### The config types

Lowering turns a validated spec into a config type, the form your program actually runs on.
`#[derive(confval::Config)]` writes that step from the spec named in `lower_from`.

```rust
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
```

- `hostname` has no attribute, so it maps across on its own.
  Lowering drops the `Located` wrapper, so `Located<String>` becomes `String`.
- `#[confval(lower(from = port, with = narrow::i64_to_u16))]` narrows the `i64` port down to a `u16`.
  [`narrow`](./guide/lowering.md#narrowing-helpers) refuses a value that does not fit instead of quietly truncating it.
- `#[confval(nested, default)]` lowers `LimitsSpec::default()` when the file left the block out, so `limits` is always set at runtime.

A `with` function has the signature `fn(&SpecField, &mut Report) -> Option<Target>`.

```rust
fn workers_to_usize(value: &Located<i64>, _report: &mut Report) -> Option<usize> {
    // Safe: the range was validated and lowering only runs on a clean report.
    Some(value.value as usize)
}
```

Lowering only runs after the gate, so a conversion here never sees a bad value.

### Running the pipeline

`main` runs the four phases in order.

First, add the source text to a [`SourceMap`](./guide/diagnostics.md#spans-and-source) and parse it.
`parse_hcl` only returns `None` when the input is broken enough that no tree comes out of it.

```rust
let mut sources = SourceMap::new();
let mut report = Report::new();
let id = sources.add("server.hcl", input);

let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);
```

Second, validate the parsed spec.

```rust
if let Some(spec) = &spec {
    validate_server(spec, &mut report);
}
```

Third, check the report.
If it holds any errors, render them and stop before lowering.

```rust
if report.has_errors() {
    let mut out = String::new();
    report.render_pretty(&sources, &mut out).unwrap();
    eprint!("{out}");
    std::process::exit(1);
}
```

Fourth, lower the validated spec into the runtime config.

```rust
let spec = spec.expect("parse returned None without reporting an error");
let config = ServerConfig::lower(&spec, &mut report).expect("validated config lowers");
```

To watch the report work, put some bad values in the input: an empty `hostname`, a `port` of `99999`, an unknown `mode`.
The gate trips, and all three come back reported, each at its own line and column.

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
