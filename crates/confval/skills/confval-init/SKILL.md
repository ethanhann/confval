---
name: confval-init
description: Scaffold a confval configuration pipeline in a Rust project that has none, building the spec, validation, and runtime layers around the project's own configuration format
---

# Set up a confval pipeline

You are adding confval to a Rust project that parses a configuration file, or that should.
confval turns a configuration file into validated runtime types, and keeps a source span on every value so a diagnostic points at the line and column the value came from.

Your job is to build three layers around the project's domain model.
The spec layer parses the file into span-tracked types.
Validation checks what the values mean and reports every problem in one pass.
The runtime layer is the resolved form the rest of the program uses.

This skill scaffolds those layers.
It does not write the project's domain rules, it does not decide the gate stage, and it does not generate schema files.
Those belong to the project.

Read the pipeline before you start.
The [pipeline reference](references/pipeline.md) covers the four phases and where a span comes from.
The [frontends reference](references/frontends.md) covers the format features and what each format can express.
The [patterns reference](references/patterns.md) covers `keyword_enum!`, `#[confval(derive_default)]`, the `narrow` helpers, and template mode.

For anything the references do not cover, read the complete confval documentation at https://ethanhann.com/confval/llms-full.txt.
That file tracks the latest release, so confirm any API against the confval version this project pins.

## Steps

### 1. Survey the project

Find where configuration already lives.
Look for an existing configuration struct, a hand-rolled parser, or a sample configuration file.
Read the format from the file extension, `.toml`, `.hcl`, `.kdl`, `.json`, or `.yaml`.
When nothing in the project settles the format, ask which one to target rather than assuming.

### 2. Add the dependency

confval has no default features, so you enable the ones the project uses.

```shell
cargo add confval@{{confval_version}} --features derive,toml
```

Enable `derive` always.
Enable the one frontend feature that matches the format, `hcl`, `toml`, `kdl`, `json`, or `yaml`.
Add `color` when the project renders diagnostics to a terminal.
Add `serde` when the project needs JSON diagnostics or a serializable config.
Add `layering` only when more than one source is combined into one configuration.
Name no feature the project does not use.

### 3. Write the spec layer

The spec is a plain struct whose every leaf field is a `Located<T>`, so each value keeps its span.
Derive the parser with `#[derive(confval::Spec)]`.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}
```

Use one nested struct per block, marked `#[confval(nested)]`.
Use `Vec<Located<T>>` for a block that may repeat.
Hold a closed set of strings as a `Located<String>` and declare its enum with `keyword_enum!`.
Add `#[confval(derive_default)]` where a block needs a `Default` built from its attribute defaults.
Store the rawest type that parses without failing, which is `String`, `i64`, `f64`, `bool`, or `PathBuf`, and narrow it later at lowering.

### 4. Write validation

Write a `Validate` impl for each spec type.
Its `validate` reports the rules for that type's own fields, and `validate_all` reaches the children, so a validator never calls a child's validator by hand.
Accumulate into the `Report` with no early return, so one run reports every problem.
Report at the offending field's span.

```rust
impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        PORT.check_located(&self.port, "port", report);
        if self.hostname.value.is_empty() {
            report
                .error("hostname must not be empty")
                .at(self.hostname.span)
                .emit();
        }
    }
}
```

Check a numeric range with a `range_constraint!` and a closed set with a keyword set.
Leave numeric narrowing and keyword-to-enum conversion to lowering, where the `narrow` helpers report an out-of-range value that a rule missed.

### 5. Write the runtime type

The config is the resolved form.
It carries no `Located` wrapper and no `Option` on a field the runtime always needs.
Use `Arc<str>` for an identifier cloned on a hot path.
Derive the mapping with `#[derive(confval::Config)]` where it is mechanical, and write a `Lower` impl where lowering can fail in a way the derive cannot express.
A lowering function reports through the `Report` and returns `None` on failure rather than panicking, which is what the `narrow` helpers already do.

```rust
#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    hostname: String,
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
    #[confval(nested, default)]
    limits: LimitsConfig,
}
```

### 6. Wire the entry point

Run the phases in order: parse, validate, gate, then lower.
The runtime path never panics, so it holds no `unwrap` and no `expect`.
A misconfiguration is reported and the program exits or rejects the reload, rather than crashing a running service.
The gate is one call, `report.has_errors()`, and it must run before lowering.
What to do when it trips, exit or reject a reload, and whether a warning also stops the run, is the project's decision.

```rust
let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);

// A syntax error yields None, with the reason already in the report. Handle it
// rather than unwrapping.
let Some(spec) = spec else {
    // render the report and stop, or reject the reload
    return;
};

spec.validate_all(&mut report);

// The gate. Stop before lowering while the report holds errors.
if report.has_errors() {
    // render the report and stop, or reject the reload
    return;
}

// A lowering error means a validation rule is missing. Report it, never panic.
if let Some(config) = ServerConfig::lower(&spec, &mut report) {
    // use config
}
```

### 7. Write a round-trip test

Add a test that parses a fixture file, validates it, lowers it, and asserts the runtime values.
Add one more case that feeds a fixture with several bad values and asserts the report names every problem in a single pass, which is the property that makes accumulation worth having.

```rust
#[test]
fn a_bad_fixture_reports_every_problem_at_once() {
    // Arrange
    let text = "hostname = \"\"\nport = 99999\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", text);

    // Act
    let spec = confval::format::toml::parse_toml(&sources, id, &mut report);
    if let Some(spec) = &spec {
        spec.validate_all(&mut report);
    }

    // Assert
    // The empty hostname and the out-of-range port are both reported, so the
    // report carries more than one issue rather than stopping at the first.
    assert!(report.has_errors());
    assert!(report.issues().len() > 1);
}
```

### 8. Verify

Run `cargo check`, then `cargo test`.
Read the spec, validation, and lowering back against the current confval crate, because the guards catch a renamed type but not advice that is merely wrong.

## Provenance

This skill was written for confval {{confval_version}}.
