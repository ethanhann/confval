---
sidebar_position: 2
---

# Validation

[Parsing](./parsing.md), which precedes validation, ensures the spec is structurally correct.
Validation is where you exhaustively check what the values mean: ranges, allowed keywords, and rules that cross more
than one field.

confval provides a `Validate` trait, described under [Validate](#validate) below.
Its main purpose is to be named in a bound on the [lower](./lowering.md) stage.
A config that opts into that bound does not compile unless its spec has a validator.
The bound guarantees a validator exists.
It does not guarantee that every field is checked inside that validator.

The checks are (currently) intentionally minimal.
confval provides only two domain-agnostic checks: `RangeConstraint` and `KeywordSet`.

## Where a rule lives

Validation rules live in one of two places.

### Validate trait implementations

A `Validate` impl on a spec type holds rules that the type can check from its own fields.
It receives `&self`, so it can read every field of that struct.
This level of granularity allows a rule spanning multiple fields to be created.

### Validator Functions

Validator functions are necessary primarily for cross-file and cross-block validation.
Depending on the domain, there may be complex semantic rules between files/blocks.
For example, imagine a server with a central configuration file that has global settings, like enabling TLS, and
subconfiguration files that may or may not be correct if TLS is enabled.
This is where validator functions come into play.

See `validate_server` in [Getting Started](../getting-started.md#the-validation-rules) for an example.

## RangeConstraint

Numeric bounds are declared once and checked against located values:

```rust
range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(DRAIN, i64, min: 0, max: 300, units: "seconds");
range_constraint!(WORKERS, i64, min: 1, max: 512, help: "Match this to your CPU core count.");

PORT.check_located(&spec.port, "port", report);
```

`check_located` emits an error at the value's span when out of range.
When **help** is provided, it overrides the auto-generated suggestion.
Otherwise, confval generates one like "Set port to at least 1".

## KeywordSet

Closed sets of allowed keyword strings are checked against located values.
This is the string counterpart of `RangeConstraint` for fields like strategies, log levels, and fail policies:

```rust
const LOAD_BALANCING_STRATEGIES: [&str; 5] =
    ["failover", "round_robin", "request_pressure", "sticky_hash", "random"];

KeywordSet::new(&LOAD_BALANCING_STRATEGIES)
    .check_located(&spec.load_balancing_strategy, "load_balancing_strategy", report);
```

`check_located` reports `unknown {field}: {value}` at the value's span, with a help line of
`expected one of: <comma-joined options>`.
Every keyword field reports the same way, so a wrong value in any closed-set field produces the same message shape and
lists the allowed set.

## Validate

`Validate` holds the semantic checks a spec type can perform on itself:

```rust
pub trait Validate {
    fn validate(&self, report: &mut Report);
}
```

A `Validate` impl checks what a spec value can prove from its own fields, reporting at the span each field already
carries.
Because it receives `&self`, it can read every field of that struct.
A rule spanning two fields of the same spec type belongs here.

It receives no span and no origin parameter, so two kinds of rule do not fit:

- A rule that must report at the span of the block itself rather than at one of its fields, such as a required child
  that is absent.
- A rule that needs something outside the struct, such as a sibling spec type or a value assembled from the whole
  configuration.

Those belong in a validator function.
Such a function holds the surrounding `Located` wrappers.
It can therefore report at any span it needs.

Beyond holding those checks, the trait gives the lowering bound something to name.
The `Config` derive, given the `validate` flag, emits it on the generated `Lower` impl:

```rust
#[derive(confval::Config)]
#[confval(lower_from = ServerSpec, validate)]
struct ServerConfig {
    /* ... */
}
// generates: impl Lower<ServerSpec> for ServerConfig where ServerSpec: Validate { ... }
```

A flagged config whose spec has no `Validate` impl then fails to compile, so a spec that can be lowered into a runtime
config but carries no validator is unrepresentable.
The flag is opt-in.
A config that does not set it is lowered with no `Validate` bound.
Handwritten `Lower` impls add the same `where S: Validate` clause directly, and a flattening lowering (one that has no
per-entity `Lower` impl) can put the bound on the function that performs it.

The bound guarantees the validator exists.
It does not guarantee that lowering calls it.
Validation is still invoked explicitly before the gate.
The trait closes the "forgot to write a validator" gap.
Calling the validator remains the caller's responsibility.
