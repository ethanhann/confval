---
sidebar_position: 2
---

# The Pipeline Contract

confval sends one configuration file through a fixed sequence of stages: **parse**, **validate**, **gate**, and **lower**.
This page is the contract for that sequence.
It defines what each stage does, in what order, and what each stage may assume about the ones before it.
The derives are designed around this ordering.

The approach is inspired by ["Parse, don't validate"](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/) by Alexis King, though it does not use newtypes to couple construction with validation.

## The four stages

### 1. Parse (structural)

A frontend (`parse_hcl`, `parse_toml`, `parse_kdl`, `parse_json`, or `parse_yaml`) builds the neutral `Fields`, runs `FromFields`, and reports shape problems.

Unknown fields, wrong types, missing required fields, and duplicate blocks are reported with spans.
Parsing continues across inputs.
An input whose tree was built keeps flowing into validation even if some of its
fields failed, so parse and validation problems appear together in one pass.
Only an input that produced no tree (a syntax error) stops the load.

### 2. Validate (semantic)

Validation checks ranges, closed sets, and cross-field rules against the spans stored in `Located` fields.
Rules live in two places: a `Validate` impl on a spec type, which takes `&self` and `&mut Report`, and validator
functions you write and call yourself.
[Validation](./guide/validation.md#where-a-rule-lives) covers which rule goes where.

One call runs the impls:

```rust
spec.validate_all(&mut report);
```

`validate_all` runs a spec type's own rules and then descends into every `#[confval(nested)]` field, recursively.
The descent comes from `#[derive(Spec)]`.
A nested block added later is therefore validated without editing a validator.
Calling `validate` instead checks the root and stops there.
[Validation](./guide/validation.md#validate-impl-contains-the-rules-validate_all-runs-them) covers the distinction.

Validation never panics.
In a system with hot reload, a panic during a reload would crash a long-lived service on a simple misconfiguration.
The issues are reported instead.

Every validator appends issues to the report.
An issue usually records a violated semantic rule rather than a handled Rust error.
For example, a date might pass the parse stage, confirming it is a date, but violate a setting-specific rule
requiring it to be at least 90 days in the future.

Spans come from the `Located` fields, so validation works the same whether the spec was parsed from a file or
constructed in code.

A spec with `#[confval(references = ...)]` fields has one more semantic check, and it reads the parsed tree rather
than `&self`.
Run `check_references` beside `validate_all`, with the parsed `Fields`, the schema, and the report:

```rust
use confval::pipeline::check_references;

check_references(&fields, &ServerSpec::schema(), &mut report);
```

The pass resolves every reference against the labels its scope can see, and reports a duplicate label, an empty
label, and a reference no label matches.
[Running the reference check](./guide/schema-ir.md#running-the-reference-check) shows the wiring, and
[How a reference resolves](./guide/schema-ir.md#how-a-reference-resolves) covers the scoping rule.

:::info
The `Validate` trait exists so the requirement can be written as a bound.
Every generated `Lower` impl carries `where SpecType: Validate + ValidateNested`, so a config does not compile unless
its spec has a validator and a traversal.

That catches the forgotten validator and the unreachable child block.
An empty `Validate` impl satisfies its half of the bound, so it does not prove any field is checked.
Neither half makes lowering call the validator, which stays an explicit step before the gate.
:::

### 3. Gate

Lowering must not run when the report contains errors.
Nothing in confval enforces this.
The caller performs the check.
Call `report.has_errors()` after validation and return before lowering when it is true.
[Getting Started](./getting-started.md#a-complete-example) shows the check in place.

Report also has `has_warnings()` and `has_issues()` (i.e., has warnings or errors).
You decide whether warnings also stop lowering.

Exit the program when the report holds errors, or reject the hot reload request.
Warnings can print without stopping either one.

### 4. Lower

`Lower::lower` converts spec types to runtime config types.

Because the gate ran, the narrowing conversions inside lowering (string to `IpNet`, `i64` to `u16`) are safe.

A failure here indicates a missing validation rule rather than invalid input.
Unlike parsing and validation, lowering does not accumulate errors.
It reports the one error and short-circuits, since a lowering error is rare and means an earlier stage let something
through.
Say so in the message.
For example, "this is likely a bug that should have been caught during validation".
An operator reading that knows the problem is in the software rather than in their configuration file.

Finally, the error still carries a span, so it renders with a source location like any other.

## Spec types vs. config types

Each setting exists in two parallel structs.

| Layer      | Derives                           | Purpose                                                       |
|------------|-----------------------------------|---------------------------------------------------------------|
| **Spec**   | `confval::Spec` (and `Serialize`) | Populated from the source file, with every field span-tracked |
| **Config** | `confval::Config` (and serde)     | Resolved, executable form used at runtime                     |

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

The generated lowering destructures the spec exhaustively, so adding a field to one side without accounting for it on
the other is a compile error.

## Type selection principle

**Spec types use the rawest type that parses infallibly**, meaning strings, `i64`, bools, and paths.
The structural parsers never reject a value for semantic reasons, so a port of `99999` or a strategy of `"failovr"`
parses fine and is caught by validation with a span, alongside every other problem.

**Keyword fields are `Located<String>` in specs.**
Closed sets like strategies or log levels are validated against a constant slice with a help line listing the options.
The runtime enum implements `TryFrom<&str>` and the conversion happens at lowering.
A serde keyword enum in the spec layer would abort parsing with a single error instead of joining the report.

**Config types use the fully parsed, typed form**, such as `IpNet`, `SocketAddr`, and runtime enums.
Downstream code never re-parses a string it received from config.

**Handwritten `FromFields` impls cover the shapes the derive does not.**
Tagged unions parse their discriminator first and dispatch.
A free-form block can be captured as an arbitrary value rather than a struct by reading the neutral field model
directly.

## Both forms normalize

Operators write nested structures either as blocks or as attribute-with-object, and real configs mix the two:

```hcl
limits {
  enable = true
}

limits = {
  enable = true
}
```

The `Fields` view normalizes both, so every nested spec accepts either form with identical spans and identical error
messages.

## Runnable examples

End-to-end examples ship in `crates/confval/examples/`.
`hcl.rs`, `toml.rs`, `kdl.rs`, `json.rs`, and `yaml.rs` each hold a source document and the two format calls that parse and emit it.
Everything after parsing lives in `common/mod.rs`: the spec types, the validators, the config types, and the lowering
functions.
All five share that file verbatim.
Every stage after parsing is in one module that all five format examples share.
The `common` module's comments explain the split.
`issue_severity.rs` reuses the same types to show a warning passing the gate.
`validate_traversal.rs` stands alone to show what `validate_all` reaches and what a `descend` override prunes.
See [Getting Started](getting-started.md) to run them.
