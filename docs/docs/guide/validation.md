---
sidebar_position: 2
---

# Validation

[Parsing](./parsing.md) ensures the spec is structurally correct.
Validation checks what the values mean: ranges, allowed keywords, and rules that span more than one field.

Two mechanisms cover the checks:

- A **recorded constraint** on a field, such as `#[confval(range = PORT)]` or `#[confval(keywords = LimitMode)]`, declares a mechanical check the derive automatically runs for you.
- A **`Validate` impl** is block-scoped. It holds the rules an attribute cannot express, such as a cross-field rule or an emptiness check. 

You call `validate_all` once on the root spec.
It runs the recorded constraints, then each type's `Validate` rules, then descends into every nested block.
It checks constraints and validation rules as the entire configuration tree is traversed.

```rust
spec.validate_all(&mut report);
```

## Declaring constraints

confval ships five domain-agnostic checks.
`RangeConstraint` bounds a number, `LengthConstraint` bounds the character count of a string, `check_format` parses a string as a named format, `KeywordSet` checks a closed string set, and `NON_EMPTY` rejects an empty value.
Each one reports at the value's span with a help line.

### RangeConstraint

`range_constraint!` declares an inclusive numeric bound:

```rust
range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(DRAIN, i64, min: 0, max: 300, units: "seconds");
range_constraint!(WORKERS, i64, min: 1, max: 512, help: "Match this to your CPU core count.");
```

`check_located` emits an error at the value's span when the value is out of range:

```rust
PORT.check_located(&spec.port, "port", report);
```

When you provide **help**, it overrides the auto-generated suggestion.
Otherwise, confval generates one like "Set port to at least 1".

### LengthConstraint

`length_constraint!` declares an inclusive bound on the character count of a string:

```rust
length_constraint!(HOSTNAME_LEN, max: 253);
length_constraint!(LABEL_LEN, min: 1, max: 63, help: "Each DNS label is at most 63 characters.");
```

A bound with `max:` alone starts at zero.
The bound takes `help:` only and has no `units:` arm, because the unit is always characters.
The count is `value.chars().count()`, the number of Unicode scalar values.
When you need a byte count, such as a DNS wire limit, write the check in the `Validate` body.

`check_located` emits an error at the value's span when the count falls outside the bound:

```rust
HOSTNAME_LEN.check_located(&spec.hostname, "hostname", report);
```

The message is "hostname must be at most 253 characters" or "hostname must be at least 1 character".
When you provide **help**, it replaces the generated suggestion.

### check_format

A format is a type that implements the `Format` trait.
The trait has a `NAME` the message uses and a `check` function that says whether a string parses.
confval ships `Ipv4`, `Ipv6`, `Ip`, and `AbsolutePath`, the formats that need nothing beyond `std`.

A domain format, such as a CIDR block or a URL, is a type you write.
For example, a CIDR block:

```rust
pub struct Cidr;

impl Format for Cidr {
    const NAME: &'static str = "CIDR block";

    fn check(value: &str) -> bool {
        let Some((address, prefix)) = value.split_once('/') else {
            return false;
        };
        let digits = !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit());
        address.parse::<std::net::Ipv4Addr>().is_ok()
            && digits
            && prefix.parse::<u8>().is_ok_and(|bits| bits <= 32)
    }
}
```

`check_format` emits an error at the value's span when the value does not parse, and `check_each_format` does the same for each element of a list:

```rust
check_format::<Ip>(&spec.hostname, "hostname", report);
check_each_format::<Cidr>(&spec.allow, "allow", report);
```

The message is `hostname is not a valid IP address: "localhost"`.
The help line is "Set hostname to a valid IP address".
On a list the message is `invalid CIDR block in allow: "10.0.0.0/33"`, which names the list rather than one element.
The value is quoted, so an empty entry reads as `""`.
`check` takes no `self`, so a format carries no configuration.
A format that needs a parameter, such as a maximum URL length, is a later extension.

### KeywordSet

`KeywordSet` checks a string value against a closed set of allowed keywords.
Use it for fields like strategies, log levels, and fail policies.

```rust
const LOAD_BALANCING_STRATEGIES: [&str; 5] =
    ["failover", "round_robin", "request_pressure", "sticky_hash", "random"];

KeywordSet::new(&LOAD_BALANCING_STRATEGIES)
    .check_located(&spec.load_balancing_strategy, "load_balancing_strategy", report);
```

`check_located` reports `unknown {field}: {value}` at the value's span, with a help line of `expected one of: <comma-joined options>`.
Every keyword field reports the same way.

#### Checking a list of keywords

`check_each` reports each bad element at its own span:

```rust
LogEvent::keyword_set().check_each(&spec.events, "event", report);
```

Name the field in the singular, because the message describes one element.
An operator who typos one entry reads `unknown event: reloded` under that entry rather than a message about the whole list.

Both list shapes pass a slice.
A bare `Vec<Located<String>>` passes itself.
A wrapped `Option<Located<Vec<Located<String>>>>` passes `&list.value`.

### keyword_enum!

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

## Recording a constraint on the field

The manual `check_located` and `check_each` calls shown above are written in a `Validate` body.
A field on a derived spec can instead record its constraint with an attribute.
The derive then runs the check for you.

`#[confval(range = PATH)]` on an `Int` or `Float` leaf, and `#[confval(keywords = PATH)]` on a `String` leaf, name the constraint the field must satisfy.
`#[confval(length = PATH)]` on a `String` leaf names a `length_constraint!` bound.
`#[confval(format = PATH)]` on a `String` leaf or a string list names a type that implements `Format`.
On a list, every element must parse.
`#[confval(keywords = PATH)]` also applies to a string list, where it records the set each element must come from.
`#[confval(non_empty)]` on a `String` leaf or a string list rejects an empty or whitespace-only value.
On an `Option<Located<String>>` leaf, the derive checks the value only when the source sets it.
On a list, it also rejects a list with zero elements.
The wrapped `Option<Located<Vec<Located<String>>>>` keeps the list's own span, so that message points at the brackets.
The bare `Vec<Located<String>>` holds no span of its own, so that message carries no location.
`validate_all` runs each recorded check.
The field needs no line in `validate`.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(non_empty, length = HOSTNAME_LEN)]
    hostname: Located<String>,
    #[confval(range = PORT)]
    port: Located<i64>,
}

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

Each attribute is the single source for its field.
It records the constraint for the [schema IR](./schema-ir.md) and runs the check.
The two cannot disagree.

A field carries at most one value constraint.
`keywords`, `range`, `length`, `format`, and `references` are the value constraints.
Two of them on one field is a compile error.
A field can carry `#[confval(non_empty)]` and one value constraint, such as `#[confval(length = ...)]`.
Pair `non_empty` with a length bound that uses `max:` alone.
`non_empty` rejects a whitespace-only value, which no length bound can express.
The two constraints then report different conditions.
`length` and `format` combine with `default`.
When the default itself fails the check, the message names the spec's default rather than the configuration.
Each built-in format rejects the empty string, so a field that carries a built-in `format` and `non_empty` reports an empty value twice.
Record `format` alone on such a field.
The pair still compiles, because a consumer format may accept the empty string.
A field cannot carry `#[confval(non_empty)]` and `#[confval(default)]` together.
The default for a string is the empty string.
The default for a list is the empty list.
Either default would fail the check.

### What recording covers

A recorded list runs `check_each_in` for a keyword set or `check_each_format` for a format.
Each bad element is reported at its own span.
The bare `Vec<Located<String>>` and the wrapped `Option<Located<Vec<Located<String>>>>` both work.
The message is `unknown value in <field>: <value>` or `invalid <format> in <field>: "<value>"`, and each one reads correctly whatever the list is called.
Call `check_each` by hand when you have a singular noun for one element, because `unknown mode: shout` is the shorter sentence.

### What stays in the Validate body

A cross-field rule has no attribute.
It stays in the `Validate` body.
An emptiness rule on a defaulted list also stays in the `Validate` body, because `non_empty` cannot be combined with `default`.

A keyword list checked by hand with `check_each` also stays there.
If you record other fields and delete that line, the check disappears with no compile error.
Record the set on the field instead, and the schema IR and the check come from one attribute.

A list of numbers has no field shape in confval.
`range` has nothing to bound on a list.
Record it on an `Int` or `Float` leaf.
`length` bounds one string, so it takes a `String` leaf alone.
A per-element bound on a string list is not recorded in this release.
`references` resolves one value against the labels in scope.
It is recorded on a scalar leaf too.

## Writing a Validate impl

`Validate` holds the semantic checks a spec type performs on itself:

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

A `Validate` impl checks what a spec value can prove from its own fields, reporting at the span each field carries.
Because it receives `&self`, it can read every field of that struct.
A rule spanning two fields of the same spec type belongs here.

Two kinds of rule do not fit:

- A rule that must report at the span of the block itself rather than at one of its fields, such as a required child that is absent.
- A rule that needs something outside the struct, such as a sibling spec type or a value assembled from the whole configuration.

Those belong in a [validator function](#validator-functions).
Such a function holds the surrounding `Located` wrappers and can report at any span it needs.

### Empty impls

An empty impl satisfies the bound.
A spec type with nothing worth checking writes one.
This states that validation was considered rather than forgotten.

### The lowering bound

The `Config` derive requires every spec to have a `Validate` impl.
It puts the bound on every generated `Lower` impl:

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
Handwritten `Lower` impls add the same `where S: Validate + ValidateNested` clause directly.

The bound guarantees that the validator exists, but it does not make lowering call it.
Validation stays an explicit step before the gate.

## Validator functions

When a rule spans more than one spec type, or needs context from outside the struct, write a validator function.

For example, imagine a server with a central configuration file that enables TLS, and subconfiguration files that depend on TLS state.
A validator function checks the agreement between them:

```rust
fn validate_tls_agreement(server: &ServerSpec, upstreams: &[UpstreamSpec], report: &mut Report) {
    /* ... */
}
```

Nothing generates these and nothing calls them for you.
They run alongside `validate_all`, before the `has_errors` check that stops the run.

## The traversal: validate vs validate_all

A `Validate` impl covers one spec type's own fields.
It does not reach the nested blocks underneath it, because those are separate types with rules of their own.

`validate_all` runs this type's `validate`, then descends into every `#[confval(nested)]` field, recursively.
One call at the root covers the whole spec tree:

```rust
spec.validate_all(&mut report);
```

An absent `Option<Located<S>>` and an empty `Vec<Located<S>>` contribute nothing to the walk.
Fields without `#[confval(nested)]` are skipped, because a scalar is checked by its own type's rules.

The traversal is a generated `ValidateNested` impl, the second half of the lowering bound.
A spec type with a handwritten `FromFields` has no derive to generate it and writes the impl itself.

:::warning
Calling `spec.validate(&mut report)` at the top of a pipeline checks the root block and leaves every nested block unchecked.
Both methods compile and both take the same arguments.
Nothing in the type system catches the mistake.

Keep `validate` out of your call sites.
The examples call `validate_all` inside the gate helper.
`validate_all` then runs in the one place that decides whether a spec is safe to lower.
:::

### Pruning a subtree with descend

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
