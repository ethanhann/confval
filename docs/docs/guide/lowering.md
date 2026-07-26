---
sidebar_position: 3
---

# Lowering

Once a spec is validated, lowering converts it into a config type: the runtime form your program uses.
Because lowering runs only after the [gate](../pipeline.md), the narrowing conversions inside it never see a bad value.

## Defining a config

`#[derive(confval::Config)]` writes the `Lower` impl that converts a validated spec into a runtime config:

```rust
#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
pub struct ServerConfig {
    #[confval(lower(from = version, with = i64_to_u32))]
    pub version: u32,

    #[confval(nested)]
    pub limits: Option<LimitsConfig>,

    pub ca_file: Option<String>,
}
```

The `Lower` trait is:

```rust
pub trait Lower<S>: Sized {
    fn lower(spec: &S, report: &mut Report) -> Option<Self>;
}
```

Field rules:

- **No attribute**: the field auto-maps via the `LowerAuto` trait, which strips `Located` wrappers without narrowing: `Located<T> -> T`, `Option<Located<T>> -> Option<T>`, `Vec<Located<T>> -> Vec<T>`, `Located<Vec<Located<T>>> -> Vec<T>`, and the optional variant of the last.
- **`#[confval(nested)]`**: the field type implements `Lower` itself.
  Works for single, `Option`, and `Vec` shapes.
- **`#[confval(nested, default)]`**: a non-optional config field lowered from an `Option<Located<S>>` spec field.
  When the source omits the block, `S::default()` is lowered in its place, so the runtime field is always populated while the spec stays source-faithful (an absent block stays `None`).
  This spelling also exists on the spec side, where it fills the omitted block during parsing instead of at lowering.
  See [Optional fields and defaults](./parsing.md#optional-fields-and-defaults) for the difference.
- **`#[confval(lower(from = field, with = fn))]`**: explicit conversion through a function `fn(&SpecField, &mut Report) -> Option<Target>`.
  All narrowing (`i64` to `u16`, string to enum, string to `IpNet`) goes through these functions.
  `from` also accepts a tuple `(a, b)` when one config field derives from several spec fields.
- **`#[confval(spec_only(field, ...))]`** at the struct level names spec fields that intentionally have no runtime counterpart.

The generated impl destructures the spec exhaustively with no rest pattern.
Adding a field to either struct without accounting for it on the other side is a compile error, which keeps spec and config in lockstep.

## Narrowing helpers

`confval::pipeline::narrow` provides ready-made `with` functions.
For integer width changes: `i64_to_u16`, `i64_to_u32`, `i64_to_u64`, `i64_to_usize`, and `opt_` variants for optional fields.
They narrow with `try_from` rather than `as`.
A value that does not fit is reported at its span and lowering fails, so a missing range rule surfaces as a located error instead of a silent truncation.
`i64_secs_to_duration` (and `opt_i64_secs_to_duration`) route a seconds count through the same checked narrow into a `Duration`, rejecting a negative value rather than wrapping it.
`i64_to_f64` widens to `f64` for the ratio and rate fields where an `as` cast cannot be named in a `with` attribute.

`keyword::<T>` lowers a validated keyword string into the enum that [`keyword_enum!`](./validation.md#keyword_enum) generates, reading that enum's `TryFrom<&str>`.
Name it with a turbofish so the derive knows which enum to parse into.
The field was validated against the same set the `TryFrom` accepts, so the conversion does not fail in a running pipeline.
The helper reports at the value's span only if the `keyword_set()` check was left out of the `Validate` impl.

```rust
use confval::pipeline::narrow;

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,

    #[confval(lower(from = mode, with = narrow::keyword::<LimitMode>))]
    mode: LimitMode,
}
```
