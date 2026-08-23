---
sidebar_position: 2
---

# Validation

[Parsing](./parsing.md) ensures the spec is structurally correct.
Validation checks what the values mean: ranges, allowed keywords, and rules that span more than one field.

confval provides a `Validate` trait, described under [Validate](#validate) below.
The trait gives the lowering bound something to name.
Every spec lowered into a config must implement it.
A config whose spec has no `Validate` impl does not compile.
The bound guarantees a validator exists.
It does not guarantee that every field is checked inside that validator.

confval ships two domain-agnostic checks, `RangeConstraint` and `KeywordSet`.

## A first validator

A spec type checks its own fields.
The two mechanical checks, a numeric range and a closed keyword set, are recorded on the field.
The derive runs them.

```rust
range_constraint!(PORT, i64, min: 1, max: 65535);
keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(range = PORT)]
    port: Located<i64>,
}

#[derive(confval::Spec)]
struct LimitsSpec {
    #[confval(keywords = LimitMode)]
    mode: Located<String>,
}
```

A rule an attribute cannot express stays in a `Validate` impl.
It reads the type's own fields and reports each problem at the field's span.
A rule that reads two fields belongs here.

You call `validate_all` once on the root spec.
It runs each type's recorded checks and its `validate`, then descends into every nested block.

```rust
spec.validate_all(&mut report);
```

[Recording a constraint on the field](#recording-a-constraint-on-the-field) covers the attributes.
The sections after it cover the handwritten rules.

## Where a rule lives

Validation rules live in one of two places: a `Validate` impl and plain validator functions.

### Validate trait implementations

A `Validate` impl on a spec type holds rules that the type can check from its own fields.
It receives `&self` and can read every field of that struct.
A rule that spans several of the struct's fields belongs here.

A nested child block is not this type's own field.
A `Validate` impl does not validate nested child specs.
You do not need to call `validate()` manually when you use `validate_all()` on the root spec.

:::info
You implement the `Validate` trait, but you call `validate_all` once on the root spec.
More information on this can be found [in this section](#validate-impl-contains-the-rules-validate_all-runs-them).
:::

### Validator functions

Validator functions handle cross-file and cross-block validation.
For example, imagine a server with a central configuration file that enables TLS, and subconfiguration files that depend on TLS state.
A validator function checks the agreement between them.

A validator function takes whatever it needs and appends to the report:

```rust
fn validate_tls_agreement(server: &ServerSpec, upstreams: &[UpstreamSpec], report: &mut Report) {
    /* ... */
}
```

Nothing generates these and nothing calls them for you.
They run alongside `validate_all`, before the `has_errors` check that stops the run.

## RangeConstraint

Numeric bounds are declared once and checked against located values:

```rust
range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(DRAIN, i64, min: 0, max: 300, units: "seconds");
range_constraint!(WORKERS, i64, min: 1, max: 512, help: "Match this to your CPU core count.");

PORT.check_located(&spec.port, "port", report);
```

`check_located` emits an error at the value's span when the value is out of range.
When you provide **help**, it overrides the auto-generated suggestion.
Otherwise, confval generates one like "Set port to at least 1".

## KeywordSet

Closed sets of allowed keyword strings are checked against located values.
Use this for fields like strategies, log levels, and fail policies:

```rust
const LOAD_BALANCING_STRATEGIES: [&str; 5] =
    ["failover", "round_robin", "request_pressure", "sticky_hash", "random"];

KeywordSet::new(&LOAD_BALANCING_STRATEGIES)
    .check_located(&spec.load_balancing_strategy, "load_balancing_strategy", report);
```

`check_located` reports `unknown {field}: {value}` at the value's span, with a help line of `expected one of: <comma-joined options>`.
Every keyword field reports the same way.

A list of keywords is checked with `check_each`, which reports each bad element at its own span:

```rust
LogEvent::keyword_set().check_each(&spec.events, "event", report);
```

Name the field in the singular, because the message describes one element.
An operator who typos one entry reads `unknown event: reloded` under that entry rather than a message about the whole list.
Both list shapes pass a slice.
A bare `Vec<Located<String>>` passes itself.
A wrapped `Option<Located<Vec<Located<String>>>>` passes `&list.value`.

## Recording a constraint on the field

The two checks above are written once in a `Validate` body.
A field on a derived spec can instead record its constraint on the field.
The derive runs the check for you.

`#[confval(range = PATH)]` on an `Int` or `Float` leaf, and `#[confval(keywords = PATH)]` on a `String` leaf, name the constraint the field must satisfy.
`#[confval(keywords = PATH)]` also applies to a string list, where it records the set each element must come from.
`validate_all` runs the recorded check, and the field needs no line in `validate`.

```rust
#[derive(confval::Spec)]
struct LimitsSpec {
    #[confval(range = MAX_BODY_MB)]
    max_body_mb: Located<i64>,
    #[confval(keywords = LimitMode)]
    mode: Located<String>,
    #[confval(keywords = LogEvent)]
    events: Vec<Located<String>>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}
```

A recorded list runs `check_each_in`.
Each bad element is reported at its own span.
The bare `Vec<Located<String>>` and the wrapped `Option<Located<Vec<Located<String>>>>` both work.
The message is `unknown value in <field>: <value>`, which reads correctly whatever the list is called.
Call `check_each` by hand when you have a singular noun for one element, because `unknown mode: shout` is the shorter sentence.

The attribute is the single source for that field.
It records the constraint for the [schema IR](./schema-ir.md) and runs the check.
The two cannot disagree.

A list of numbers has no field shape in confval.
`range` has nothing to bound on a list.
Record it on an `Int` or `Float` leaf.
`references` resolves one value against the labels in scope.
It is recorded on a scalar leaf too.

A cross-field rule and an emptiness check have no attribute.
They stay in the `Validate` body.
A keyword list checked by hand with `check_each` also stays there.
If you record other fields and delete that line, the check disappears with no compile error.
Record the set on the field instead, and the schema IR and the check come from one attribute.

## keyword_enum!

A closed-set field normally requires three declarations: a `const` slice of allowed strings for the `KeywordSet` check, a runtime enum, and a `TryFrom<&str>` impl that bridges them at lowering.
Nothing keeps the three in agreement.
`keyword_enum!` declares all three from one table:

```rust
keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});
```

The keyword on the right of each arrow is the single source of truth.
The macro generates the following items for the visibility you give it:

| Generated item | Description |
|---|---|
| `enum LimitMode` | Derives `Debug, Clone, Copy, PartialEq, Eq` |
| `LimitMode::KEYWORDS` | The allowed set as a `&[&str]` |
| `LimitMode::keyword_set()` | Returns a `KeywordSet` for the allowed strings |
| `as_str` | Returns the keyword string for a variant |
| `TryFrom<&str>` | Accepts exactly the declared keywords |
| `Display` | Writes the keyword string |
| `Serialize` (with `serde` feature) | Writes the keyword string, e.g. `"log"` rather than the Rust variant name `Log` |

:::caution
If you already wrote a `Serialize` impl for the enum, remove it.
The two impls conflict.
:::

You check a keyword field in one of two ways.
On a derived spec, record the set on the field and let the derive run the check:

```rust
#[derive(confval::Spec)]
struct LimitsSpec {
    #[confval(keywords = LimitMode)]
    mode: Located<String>,
}
```

On a handwritten spec, or from a validator function, call the accessor yourself:

```rust
LimitMode::keyword_set().check_located(&self.mode, "mode", report);
```

Either way, a value that fails the check never reaches the `TryFrom`.
The enum and its allowed set cannot drift.
To lower the validated string into the enum, name `narrow::keyword::<LimitMode>` as the `with` function.
The [lowering](./lowering.md#narrowing-helpers) guide covers that helper.

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
It is the one you implement.
The other two are covered [below](#validate-impl-contains-the-rules-validate_all-runs-them).

### What belongs in a Validate impl

A `Validate` impl checks what a spec value can prove from its own fields, reporting at the span each field carries.
Because it receives `&self`, it can read every field of that struct.
A rule spanning two fields of the same spec type belongs here.

Two kinds of rule do not fit:

- A rule that must report at the span of the block itself rather than at one of its fields, such as a required child that is absent.
- A rule that needs something outside the struct, such as a sibling spec type or a value assembled from the whole configuration.

Those belong in a validator function.
Such a function holds the surrounding `Located` wrappers and can report at any span it needs.

### The lowering bound

The `Config` derive puts a `Validate` bound on every generated `Lower` impl:

```rust
#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    /* ... */
}
// generates: impl Lower<ServerSpec> for ServerConfig
//   where ServerSpec: Validate + ValidateNested { ... }
```

A config whose spec has no `Validate` impl does not compile.
A spec that can be lowered into a runtime config but has no validator is unrepresentable.

### Empty impls

An empty impl satisfies the bound.
A spec type with nothing worth checking writes one.
This states that validation was considered rather than forgotten.

Handwritten `Lower` impls add the same `where S: Validate + ValidateNested` clause directly.
A flattening lowering can put the bound on the function that performs it.

The bound guarantees that the validator exists, but it does not make lowering call it.
Validation stays an explicit step before the gate.

## `Validate` impl contains the rules, `validate_all` runs them

A `Validate` impl covers one spec type's own fields.
It does not reach the nested blocks underneath it, because those are separate types with rules of their own.
The specs for a configuration surface form a tree.
A traversal has to visit every node.

`validate_all` runs this type's `validate`, then descends into every `#[confval(nested)]` field, recursively.
One call at the root covers the whole spec tree:

```rust
spec.validate_all(&mut report);
```

An absent `Option<Located<S>>` and an empty `Vec<Located<S>>` contribute nothing to the walk.
Fields without `#[confval(nested)]` are skipped, because a scalar is checked by its own type's rules.

The traversal is a generated `ValidateNested` impl, the second half of the lowering bound shown above.
A spec type with a handwritten `FromFields` has no derive to generate it and writes the impl itself.

:::warning
Calling `spec.validate(&mut report)` at the top of a pipeline checks the root block and leaves every nested block unchecked.
Both methods compile and both take the same arguments.
Nothing in the type system catches the mistake.

Keep `validate` out of your call sites.
The examples call `validate_all` inside the gate helper.
`validate_all` then runs in the one place that decides whether a spec is safe to lower.
:::

### Pruning a subtree with `descend`

Sometimes a block turns off the feature it configures while its child blocks remain in the file.
If the traversal validates those children, the operator receives errors about settings that are not in use.
Those errors have to be separated from the ones that apply to the running configuration.

The `descend` method lets a spec type skip its own children.
It runs after `validate` and returns a `ControlFlow` value:

- `ControlFlow::Continue(())`, the default, validates every nested child.
- `ControlFlow::Break(())` stops the descent and leaves the children unvisited.

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

`descend` is also useful when a spec has reported that the file was written for a different schema version.
The operator needs to correct the version before the individual field errors are worth reading.

Because `descend` runs after `validate`, anything the type reported about itself stays in the report.
Only the children are skipped.

The `validate_traversal` example runs the same invalid configuration twice, changing only the `enable` field, and prints both reports.
