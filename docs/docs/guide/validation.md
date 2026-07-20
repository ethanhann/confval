---
sidebar_position: 3
---

# Validation

Parsing gets you a spec with the right shape.
Validation is where you check what the values mean: ranges, allowed keywords, and rules that cross more than one field.
It runs against the span each `Located` field already carries, so every message points back at the file.

confval gives you two ready-made checks and a `Validate` trait.

## RangeConstraint

Numeric bounds are declared once and checked against located values:

```rust
range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(DRAIN, i64, min: 0, max: 300, units: "seconds");
range_constraint!(WORKERS, i64, min: 1, max: 512, help: "Match this to your CPU core count.");

PORT.check_located(&spec.port, "port", report);
```

`check_located` emits an error at the value's span when out of range.
When **help** is provided it overrides the auto-generated suggestion.
Otherwise confval generates one like "Set port to at least 1".

## KeywordSet

Closed sets of allowed keyword strings are checked against located values.
This is the string counterpart of `RangeConstraint` for fields like strategies, log levels, and fail policies:

```rust
const LOAD_BALANCING_STRATEGIES: [&str; 5] =
    ["failover", "round_robin", "request_pressure", "sticky_hash", "random"];

KeywordSet::new(&LOAD_BALANCING_STRATEGIES)
    .check_located(&spec.load_balancing_strategy, "load_balancing_strategy", report);
```

`check_located` reports `unknown {field}: {value}` at the value's span, with a help line of `expected one of: <comma-joined options>`.
Every keyword field reports the same way, so a wrong value in any closed-set field produces the same message shape and lists the allowed set.

## Validate

`Validate` is field-local semantic validation for a spec type:

```rust
pub trait Validate {
    fn validate(&self, report: &mut Report);
}
```

A `Validate` impl checks what a value can prove about itself from its own fields, reporting at the span each field already carries.
It takes only `&self` and the report: no span and no origin parameter, because anything needing more context (a missing required child, a cross-field rule, a relational check across the whole config) belongs in the consumer's central validators, not here.

The trait's reason to exist is to be nameable in a bound.
The `Config` derive, given the `validate` flag, emits it on the generated `Lower` impl:

```rust
#[derive(confval::Config)]
#[confval(lower_from = ServerSpec, validate)]
struct ServerConfig {
    /* ... */
}
// generates: impl Lower<ServerSpec> for ServerConfig where ServerSpec: Validate { ... }
```

A flagged config whose spec has no `Validate` impl then fails to compile, so a spec that can be lowered into a runtime config but carries no validator is unrepresentable.
The flag is opt-in: configs that do not request it lower exactly as before.
Hand-written `Lower` impls add the same `where S: Validate` clause directly, and a flattening lowering (one that has no per-entity `Lower` impl) can put the bound on the function that performs it.

The bound guarantees the validator exists, not that lowering calls it.
Validation is still invoked explicitly before the gate.
The trait closes the "forgot to write a validator" gap, and the call site remains the consumer's responsibility.
