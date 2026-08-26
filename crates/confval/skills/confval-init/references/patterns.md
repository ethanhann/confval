# The patterns you use

You are writing the spec, validation, and lowering, and a few recurring shapes cover most of what a spec needs.
This file gives each one its declaration, one use, and the situation that calls for it.

## A closed set of strings

Sometimes a field accepts one of a fixed list of words, such as a mode or a log level.
The spec holds it as a `Located<String>`, validation checks it against a keyword set, and lowering converts it to an enum.
`keyword_enum!` declares that enum and the set together.

```rust
keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});
```

The macro generates the enum, a `KEYWORDS` array, `as_str`, a `TryFrom<&str>`, a `Display`, and a `keyword_set()`.
Declare the check on the field with `#[confval(keywords = LimitMode)]`.
The derive runs it during validation and records it in the schema.
An editor's hover and completion read that same rule.
Your `Validate` body carries no line for the field.
Lowering names `narrow::keyword::<LimitMode>` to read the `TryFrom`.
Once the check is in place, a value that fails it never reaches the `TryFrom`, so the set and the enum cannot drift.

When you check a set inline rather than from an enum, `KeywordSet` is the same check over a bare slice.

```rust
use confval::prelude::{KeywordSet, Located, Report};

const STRATEGIES: [&str; 2] = ["round_robin", "least_conn"];
KeywordSet::new(&STRATEGIES).check_located(&strategy, "strategy", &mut report);
```

## A string with a length bound

When a string has a maximum length, such as a hostname or a DNS label, declare the bound with `length_constraint!` and record it on the field with `#[confval(length = ...)]`.
The count is in characters.

```rust
length_constraint!(HOSTNAME_LEN, max: 253);

#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(non_empty, length = HOSTNAME_LEN)]
    hostname: Located<String>,
}
```

A bound with `max:` alone starts at zero.
It then pairs with `non_empty`, and the two report different conditions.
Like `range_constraint!`, the macro generates a private const, so declare it in the module that declares the spec struct.
The derive rejects `length` on a list, a map, a block, and a non-string leaf.

## A string that must parse as a format

When a string must parse as one kind of value, such as an IP address or an absolute path, record the format on the field with `#[confval(format = ...)]`.
`Ipv4`, `Ipv6`, `Ip`, and `AbsolutePath` ship with the crate.
On a list the format applies to each element.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(format = Ip)]
    bind: Located<String>,
    #[confval(default, format = Ip)]
    peers: Vec<Located<String>>,
}
```

A domain format is a unit struct that implements `Format` with a `NAME` and a `check` function.
Declare it in a module the spec module can import from, because `format = ...` names a type.
Do not pair a built-in `format` with `non_empty`, because each built-in format rejects the empty string on its own.

## A list with no repeated entry

When a list must not repeat an entry, such as a list of route paths or of allowed networks, record it on the field with `#[confval(unique)]`.
The derive reports each repeat at its own span and leaves the first occurrence alone.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(default, unique, format = Ip)]
    peers: Vec<Located<String>>,
}
```

`unique` combines with `keywords`, `format`, `non_empty`, and `default`, because the default list is empty and so unique.
A duplicate check that spans blocks, such as a service name unique across files, compares labels and stays in the `Validate` body.

## A non-empty string or list

When a field must not be empty, such as a service name or a list of allowed networks, record it on the field with `#[confval(non_empty)]`.
The derive rejects an empty or whitespace-only string.
On a list it rejects each empty element and a list with no elements.
The list-level message points at the brackets for an `Option<Located<Vec<Located<String>>>>` field.
A bare `Vec<Located<String>>` holds no span of its own, so its list-level message carries no location.

```rust
#[derive(confval::Spec)]
struct ServiceSpec {
    #[confval(non_empty)]
    name: Located<String>,
}
```

A field can carry `#[confval(non_empty)]` and one value constraint, such as `#[confval(keywords = ...)]`.
A field cannot carry `#[confval(non_empty)]` and `#[confval(default)]` together, because the default is empty and would fail the check.

For a handwritten spec, call the checker directly:

```rust
use confval::prelude::{NON_EMPTY, Located, Report};

NON_EMPTY.check_located(&name, "name", &mut report);
```

## A default declared once

When a field has a default, the parser fills it when the source omits the field.
When a whole block is optional and marked to fill, the config side lowers the block's `Default`, so the spec type needs a `Default` impl.
Writing that impl by hand repeats the attribute defaults.
Nothing keeps the two in agreement.

`#[confval(derive_default)]` on the struct generates the `Default` impl from the attribute defaults, so each default is declared once.

```rust
#[derive(confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}
```

A spec whose every field is optional and declares no default may use `#[derive(Default)]`, because all `None` is what the parser fills.
Nothing is declared twice, so nothing can drift.
A spec where any field declares `#[confval(default = ...)]` uses `#[confval(derive_default)]`.

The standard derive fills an undeclared field with `T::default()` without reporting it, so the value for an absent block and the value for an omitted field can drift apart.
`#[confval(derive_default)]` refuses a field that declares no default rather than inventing a value, which keeps the two the same.
A handwritten `impl Default` that repeats the attribute values reintroduces the same drift by hand.
A migration from an earlier version usually leaves one behind.
Delete it and add the attribute.

## The configuration with no file

A program that runs without a configuration file still needs a config.
Lower the spec's own `Default` to get it.
The defaults then have one declaration site, the attributes, and this path reads the same ones the parser fills.

```rust
let spec = ServerSpec::default();
let mut report = Report::new();
spec.validate_all(&mut report);
if report.has_errors() {
    return None;
}
ServerConfig::lower(&spec, &mut report)
```

Do not declare the defaults a second time as constants, and do not write a runtime `Default` by hand.
Either one gives a value two declaration sites that nothing keeps in agreement.
A cast in a handwritten runtime `Default` is a sign of this, because it repeats the narrowing that lowering already does.

## Narrowing at the lowering boundary

A config field whose type already matches the spec field needs no attribute.
It auto-maps and strips the `Located` wrapper, so `Located<T>` becomes `T` and `Option<Located<T>>` becomes `Option<T>`.
Reserve `with` for a field that narrows or parses.

A spec stores every integer as `i64`, the widest a source format produces, and a runtime type uses the width it needs.
The `narrow` helpers convert between the two and slot directly into `#[confval(lower(from = ..., with = ...))]`.
Each reports at the value's span and returns `None` when the value does not fit, because a value that does not fit means a validation rule is missing rather than that the operator made a typo.

| Helper | Case it covers |
|--------|----------------|
| `narrow::i64_to_u16`, `i64_to_u32`, `i64_to_u64`, `i64_to_usize` | a required integer field narrows to a fixed width |
| `narrow::opt_i64_to_u16` and the other `opt_` forms | the field is `Option<Located<i64>>` |
| `narrow::i64_to_f64` | a rate or ratio widens to `f64`, which cannot fail |
| `narrow::i64_secs_to_duration`, `opt_i64_secs_to_duration` | a seconds count becomes a `Duration` |
| `narrow::keyword::<T>` | a validated keyword string becomes its enum |
| `narrow::keyword_list::<T>`, `opt_keyword_list::<T>` | a list of keyword strings becomes a list of enum values |

The `keyword` helper needs a turbofish so the derive knows which enum to parse into.

```rust
#[confval(lower(from = mode, with = narrow::keyword::<LimitMode>))]
mode: LimitMode,
```

## Template mode

A spec already names every field, holds every default, and carries the doc comment you wrote on each field, so you can run parsing backward and write a starter file.
`to_fields` produces the configuration with every default filled in.
`to_template` produces the same file with each field's doc comment rendered as a comment above it.

```rust
let dump = confval::format::toml::emit_toml(&spec.to_fields())?;
let annotated = confval::format::toml::emit_toml(&spec.to_template())?;
```

Use template mode when you build a command that writes a starter config or shows what the spec resolved to once its defaults applied.
An optional block is filled in a template only when you mark it `#[confval(nested, default)]`, which fills it from its type's `Default`.
JSON has no comment syntax, so a JSON template equals the plain dump.

## A repeated block whose instances must be unique

A `Vec<Located<T>>` accepts a repeated block, and nothing in the field shape says the instances differ.
Uniqueness on a name, an address, or a path is a rule you write.
Report the second occurrence at its own span, and point at the first with `.related`.

```rust
fn report_duplicate<K: Eq + Hash>(
    seen: &mut HashMap<K, Span>,
    key: K,
    span: Span,
    report: &mut Report,
    message: impl FnOnce() -> String,
) {
    match seen.get(&key) {
        Some(first) => report
            .error(message())
            .at(span)
            .related(*first, "first declared here")
            .emit(),
        None => {
            seen.insert(key, span);
        }
    }
}
```

Build the message through a closure, so a load with no duplicate never formats it.
One helper covers every key a spec keeps unique, whatever the key type.

## A block that no longer applies

`Validate::descend` decides whether the children of a block are checked.
The default continues, so a whole subtree is checked without anyone asking for it.
Break the descent when the block has declared itself inapplicable, because the children's diagnostics would be noise rather than help.

```rust
impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        if self.version.value != SCHEMA_VERSION {
            report
                .error(format!("unknown config version: {}", self.version.value))
                .at(self.version.span)
                .help("This build reads a different schema. Upgrade the program.")
                .emit();
            return;
        }
        // the rules for this version
    }

    fn descend(&self) -> ControlFlow<()> {
        if self.version.value == SCHEMA_VERSION {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }
}
```

`descend` runs after `validate`, so whatever the block reported about itself survives the pruning of its subtree.
A disabled feature whose sub-blocks no longer mean anything is the other common case.

A gate that stops the descent does not stop a recorded check.
`validate_all` runs every recorded check before `validate`, so a `range` or `keywords` attribute fires even when the gate has already reported that the block does not apply.
Keep a check that must stay silent behind the gate in `validate`, guarded by the same condition, rather than on the field.

## A labeled block another block references

Sometimes a repeated block names its instances, and another field points at one by name.
`#[confval(label)]` marks the child field that holds the name, and the HCL and KDL frontends read the native label syntax, so `upstream "api" { ... }` fills the marked field.
`#[confval(references = upstream)]` marks a string field whose value must name one of those labels, where `upstream` is the parent's field name for the labeled block.
After parsing, call `check_references` with the parsed fields, the schema, and the report.
The pass reports a reference that no label in scope matches.

```rust
#[derive(confval::Spec)]
struct GatewaySpec {
    #[confval(nested)]
    upstream: Vec<Located<UpstreamSpec>>,
    #[confval(nested)]
    rules: Vec<Located<RuleSpec>>,
}

#[derive(confval::Spec)]
struct UpstreamSpec {
    #[confval(label)]
    name: Located<String>,
    host: Located<String>,
}

#[derive(confval::Spec)]
struct RuleSpec {
    prefix: Located<String>,
    #[confval(references = upstream)]
    upstream: Located<String>,
}
```

## The patterns together

This program declares an enum with `keyword_enum!`, records both constraints on their fields, derives `Default` from the attribute defaults, and lowers with the `narrow` helpers.

```rust
use confval::prelude::*;

range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

#[derive(confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    #[confval(default = 16, range = MAX_BODY_MB)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string(), keywords = LimitMode)]
    mode: Located<String>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Config)]
#[confval(lower_from = LimitsSpec)]
struct LimitsConfig {
    #[confval(lower(from = max_body_mb, with = narrow::i64_to_u16))]
    max_body_mb: u16,
    #[confval(lower(from = mode, with = narrow::keyword::<LimitMode>))]
    mode: LimitMode,
}

fn main() {
    let spec = LimitsSpec::default();
    let mut report = Report::new();
    spec.validate_all(&mut report);
    // Handle the lowering result rather than unwrapping.
    if let Some(config) = LimitsConfig::lower(&spec, &mut report) {
        println!("{} {}", config.max_body_mb, config.mode);
    }
}
```
