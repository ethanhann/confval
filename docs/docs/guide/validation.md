---
sidebar_position: 2
---

# Validation

[Parsing](./parsing.md), which precedes validation, ensures the spec is structurally correct.
Validation is where you exhaustively check what the values mean: ranges, allowed keywords, and rules that cross more
than one field.

confval provides a `Validate` trait, described under [Validate](#validate) below.
Its main purpose is to be named in a bound on the [lower](./lowering.md) stage.
Every spec lowered into a config must implement it, or the config does not compile.
The bound guarantees a validator exists.
It does not guarantee that every field is checked inside that validator.

The checks are (currently) intentionally minimal.
confval provides only two domain-agnostic checks: `RangeConstraint` and `KeywordSet`.

## Concept Overview

This is a high-level look at validation.
The sections below cover each part in more detail.

A spec type checks its own fields in a `Validate` impl.
Numeric bounds use `RangeConstraint`, closed sets use `KeywordSet`, and each problem is reported at the field's span.

```rust
range_constraint!(PORT, i64, min: 1, max: 65535);
keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        PORT.check_located(&self.port, "port", report);
    }
}

impl Validate for LimitsSpec {
    fn validate(&self, report: &mut Report) {
        LimitMode::keyword_set().check_located(&self.mode, "mode", report);
    }
}
```

You call `validate_all` once on the root spec.
It runs each type's `validate` and descends into every nested block.

```rust
spec.validate_all(&mut report);
```

## Where a rule lives

Validation rules live in one of two places: a `Validate` impl and plain validator functions (when necessary).

### Validate trait implementations

A `Validate` impl on a spec type holds rules that the type can check from its own fields.
It receives `&self`, so it can read every field of that struct.
A rule that spans several of the struct's fields can therefore live here.

A nested child block is not this type's own field.
A `Validate` impl therefore does not itself validate nested child specs.
However, there is also no need to call `validate()` manually if you use `validate_all()` on the root spec.

:::info
You implement the `Validate` trait, but you call `validate_all` once on the root spec.
More information on this can be found [in this section](#validate-impl-contains-the-rules-validate_all-runs-them).
:::

### Validator Functions

Validator functions are necessary primarily for cross-file and cross-block validation.
Depending on the domain, there may be complex semantic rules between files/blocks.
For example, imagine a server with a central configuration file that has global settings, like enabling TLS, and
subconfiguration files that may or may not be correct if TLS is enabled.
A validator function handles this case.

A validator function takes whatever it needs to check and appends to the report:

```rust
fn validate_tls_agreement(server: &ServerSpec, upstreams: &[UpstreamSpec], report: &mut Report) {
    /* ... */
}
```

Nothing generates these and nothing calls them for you.
They run alongside `validate_all`, before the gate.

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

## keyword_enum!

A closed-set field is otherwise declared three times.
A `const` slice of allowed strings feeds the `KeywordSet` check.
A runtime enum holds the value the program runs on.
A `TryFrom<&str>` impl bridges the two at lowering.
Nothing keeps the three in agreement, so a variant added to one and not the others drifts.

`keyword_enum!` declares all three from one table:

```rust
keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});
```

The keyword on the right of each arrow is the single source of truth.
For the visibility you give it, the macro generates the enum (deriving `Debug, Clone, Copy, PartialEq, Eq`), the allowed
set as `LimitMode::KEYWORDS`, a `LimitMode::keyword_set()` accessor, `as_str`, a `TryFrom<&str>` that accepts exactly the
keywords, and `Display`.
With confval's `serde` feature enabled it also generates a `Serialize` impl that writes the keyword string, so a
serialized config spells `"log"` rather than the Rust variant name `Log`.
If you already wrote a `Serialize` for the enum yourself, remove it, because the two impls conflict.

You validate a keyword field through the accessor:

```rust
impl Validate for LimitsSpec {
    fn validate(&self, report: &mut Report) {
        LimitMode::keyword_set().check_located(&self.mode, "mode", report);
    }
}
```

The one artifact the macro does not generate is that `keyword_set().check_located(...)` call.
Wire it into the `Validate` impl.
A value that fails the check then never reaches the `TryFrom`, so the enum and its allowed set cannot drift.
To lower the validated string into the enum, name `narrow::keyword::<LimitMode>` as the `with` function, which the
[lowering](./lowering.md#narrowing-helpers) guide covers.

## Validate

`Validate` holds the semantic checks a spec type can perform on itself:

```rust
pub trait Validate {
    fn validate(&self, report: &mut Report);

    fn descend(&self) -> ControlFlow<()> { /* ... */ }

    fn validate_all(&self, report: &mut Report) where
        Self: ValidateNested
    { /* ... */ }
}
```

`validate` is the only method with no default.
It is the one to implement.
The other two are covered [below](#validate-impl-contains-the-rules-validate_all-runs-them).

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
The `Config` derive puts that bound on every generated `Lower` impl:

```rust
#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    /* ... */
}
// generates: impl Lower<ServerSpec> for ServerConfig
//   where ServerSpec: Validate + ValidateNested { ... }
```

A config whose spec has no `Validate` impl fails to compile.
A spec that can be lowered into a runtime config but carries no validator is therefore unrepresentable.

An empty impl satisfies the bound.
A spec type with nothing worth checking writes one, which states that validation was considered rather than forgotten.

Handwritten `Lower` impls add the same `where S: Validate + ValidateNested` clause directly.
A flattening lowering, meaning one with no per-entity `Lower` impl, can put the bound on the function that performs it.

The bound guarantees the validator exists.
It does not guarantee that lowering calls it.
Validation is still invoked explicitly before the gate.
The trait closes the "forgot to write a validator" gap.
Calling the validator remains the caller's responsibility.

## `Validate` impl contains the rules, `validate_all` runs them

A `Validate` impl covers one spec type's own fields.
It does not reach the nested blocks underneath it, because those are separate types with rules of their own.
The specs for a configuration surface form a tree.
Something has to walk the tree.
That walk is generated rather than written by hand, though you can write it yourself.

`validate_all` runs this type's `validate`, then descends into every `#[confval(nested)]` field, recursively.
One call at the root therefore covers the whole spec tree:

```rust
spec.validate_all(&mut report);
```

An absent `Option<Located<S>>` and an empty `Vec<Located<S>>` contribute nothing to the walk.
Fields without `#[confval(nested)]` are skipped, because a scalar is checked by its own type's rules or not checked.

The traversal itself is a generated `ValidateNested` impl, which is the second half of the lowering bound shown above.
A spec type with a handwritten `FromFields` has no derive to generate it and writes the impl itself.

:::warning
Calling `spec.validate(&mut report)` at the top of a pipeline checks the root block and leaves every nested block
unchecked.
Nothing in the type system catches that, because both methods compile and both take the same arguments.

Keep `validate` out of your call sites.
The examples fold validation into the gate helper.
`validate_all` then runs in the one place that decides whether a spec is safe to lower.
:::

### Pruning a subtree with `descend`

Sometimes a block turns off the feature it configures while its child blocks remain in the file.
If the traversal validates those children, the operator receives errors about settings that will not be used.
Those errors then have to be separated from the ones that apply to the running configuration.

The `descend` method lets a spec type skip its own children.
It runs after `validate` and returns a `ControlFlow` value:

- `ControlFlow::Continue(())`, the default, validates every nested child.
- `ControlFlow::Break(())` stops, leaving the children unvisited.

For example, an `UpstreamSpec` that has been disabled may skip the retry and timeout blocks beneath it:

```rust
impl Validate for UpstreamSpec {
    fn validate(&self, report: &mut Report) {
        /* ... */
    }

    fn descend(&self) -> ControlFlow<()> {
        if self.enable.value {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }
}
```

You may also find `descend` useful when a spec has already reported that the file was written for a different schema
version.
The operator needs to correct the version before the individual field errors are worth reading.

Because `descend` runs after `validate`, anything the type reported about itself stays in the report.
Only the children are skipped.

The `validate_traversal` example runs the same invalid configuration twice, changing only the `enable` field, and prints
both reports.
