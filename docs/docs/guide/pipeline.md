---
sidebar_position: 6
---

# The pipeline contract

confval assumes a fixed phase ordering, and the derives are designed around it.
A configuration file moves through four ordered phases: parse, validate, gate, and lower.

## The four phases

1. **Parse** (structural).
   A frontend (`parse_hcl` or `parse_toml`) builds the neutral `Fields`, runs `FromFields`, and reports shape problems.
   Unknown fields, wrong types, missing required fields, and duplicate blocks are reported with spans.
   Parsing continues across inputs: an input whose tree was built keeps flowing into validation even if some of its fields failed, so the operator sees parse and validation problems in one pass.
   Only an input that produced no tree at all (a syntax error) stops the load.

2. **Validate** (semantic).
   `Validate` impls take `&self` and `&mut Report` and check ranges, closed sets, and cross-field rules against the spans stored in `Located` fields.
   Validation never panics and never returns early.
   Every validator appends issues to the report.
   Spans come from the `Located` fields, so validation works the same whether the spec was parsed from a file or constructed in code.
   The trait doubles as a compile-time bound on the lower phase, so a lowerable spec without a validator does not compile.

3. **Gate**.
   If the report contains any errors after validation, lowering must not run.
   Warnings alone do not block.

4. **Lower**.
   `Lower::lower` converts spec types to runtime config types.
   Because the gate ran, the narrowing conversions inside lowering (string to `IpNet`, `i64` to `u16`) are safe.
   A failure here indicates a missing validation rule, not bad operator input, and it still reports through the same span pipeline so even a missing-rule bug renders with a source location.

## Spec types vs config types

Each setting exists in two parallel structs.

| Layer      | Derives                        | Purpose                                                |
|------------|--------------------------------|--------------------------------------------------------|
| **Spec**   | `confval::Spec` (and `Serialize`) | Populated from the source file; every field is span-tracked |
| **Config** | `confval::Config` (and serde)  | Resolved, executable form used at runtime              |

Spec fields are wrapped in `Located<T>`:

```rust
#[derive(Debug, confval::Spec)]
pub struct ServerSpec {
    pub version: Located<i64>,
    pub threads: Option<Located<i64>>,

    #[confval(nested)]
    pub limits: Option<Located<LimitsSpec>>,

    #[confval(default = 30)]
    pub refresh_interval_seconds: Located<i64>,
}
```

Config structs declare how each field lowers:

```rust
#[derive(Debug, Clone, confval::Config)]
#[confval(lower_from = ServerSpec)]
pub struct ServerConfig {
    #[confval(lower(from = version, with = i64_to_u32))]
    pub version: u32,

    #[confval(nested)]
    pub limits: Option<LimitsConfig>,

    pub ca_file: Option<String>,  // auto-mapped, Located stripped
}
```

The generated lowering destructures the spec exhaustively, so adding a field to one side without accounting for it on the other is a compile error.

## Type selection principle

**Spec types use the rawest type that parses infallibly.**
Strings, `i64`, bools, paths.
The structural parsers never reject a value for semantic reasons, so a port of `99999` or a strategy of `"failovr"` parses fine and is caught by validation with a span, alongside every other problem.

**Keyword fields are `Located<String>` in specs.**
Closed sets like strategies or log levels are validated against a constant slice with a help line listing the options.
The runtime enum implements `TryFrom<&str>` and the conversion happens at lowering.
A serde keyword enum in the spec layer would abort parsing with a single error instead of joining the report.

**Config types use the fully parsed, typed form.**
`IpNet`, `SocketAddr`, runtime enums.
Downstream code never re-parses a string it received from config.

**Hand-written `FromFields` impls cover the shapes the derive does not.**
Tagged unions parse their discriminator first and dispatch.
A free-form block can be captured as an arbitrary value rather than a struct by reading the neutral field model directly.

## Both spellings normalize

Operators write nested structures either as blocks or as attribute-with-object, and real configs mix the two:

```hcl
limits {
  enable = true
}

limits = {
  enable = true
}
```

The `Fields` view normalizes both, so every nested spec accepts either spelling with identical spans and identical error messages.

## Runnable examples

Two end-to-end examples ship in `crates/confval/examples/`: `hcl.rs` and `toml.rs`.
They define the same `ServerSpec` and `ServerConfig` and differ only in the source text and the single `parse_hcl` vs `parse_toml` call, demonstrating that everything after parsing is format-neutral.
See [Getting Started](../getting-started.md) to run them.
