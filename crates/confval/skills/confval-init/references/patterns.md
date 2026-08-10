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

The macro generates the enum, a `KEYWORDS` array, `as_str`, a `TryFrom<&str>`, a `Display`, and a `keyword_set()` you call from a `Validate` impl.
It does not generate the check itself.
You write `LimitMode::keyword_set().check_located(&self.mode, "mode", report)` in your validator, and lowering names `narrow::keyword::<LimitMode>` to read the `TryFrom`.
Once the check is in place, a value that fails it never reaches the `TryFrom`, so the set and the enum cannot drift.

When you check a set inline rather than from an enum, `KeywordSet` is the same check over a bare slice.

```rust
use confval::prelude::{KeywordSet, Located, Report};

const STRATEGIES: [&str; 2] = ["round_robin", "least_conn"];
KeywordSet::new(&STRATEGIES).check_located(&strategy, "strategy", &mut report);
```

## A default declared once

When a field has a default, the parser fills it when the source omits the field.
When a whole block is optional and marked to fill, the config side lowers the block's `Default`, so the spec type needs a `Default` impl.
Writing that impl by hand repeats the attribute defaults, and nothing keeps the two in agreement.

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

Use it rather than `#[derive(Default)]` on a spec.
The standard derive fills an undeclared field with `T::default()` without reporting it, so the value for an absent block and the value for an omitted field can drift apart.
`#[confval(derive_default)]` refuses a field that declares no default rather than inventing a value, which keeps the two the same.

## Narrowing at the lowering boundary

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

## The patterns together

This program declares an enum with `keyword_enum!`, derives `Default` from the attribute defaults, validates the fields, and lowers with the `narrow` helpers.

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
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    mode: Located<String>,
}

impl Validate for LimitsSpec {
    fn validate(&self, report: &mut Report) {
        MAX_BODY_MB.check_located(&self.max_body_mb, "max_body_mb", report);
        LimitMode::keyword_set().check_located(&self.mode, "mode", report);
    }
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
