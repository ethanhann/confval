---
sidebar_position: 4
---

# Derive macros

The `derive` feature provides two macros from the `confval-derive` crate.
The macros emit format-neutral code, so `derive` does not imply any frontend.
Pair it with `hcl` and/or `toml`.

## #[derive(confval::Spec)]

Generates the `FromFields` impl for a spec struct.
Parsing is purely structural.
The macro never embeds semantic rules.

```rust
#[derive(confval::Spec)]
pub struct ServerSpec {
    pub version: Located<i64>,
    pub threads: Option<Located<i64>>,

    #[confval(nested)]
    pub limits: Option<Located<LimitsSpec>>,

    #[confval(default = 30)]
    pub refresh_interval_seconds: Located<i64>,
}
```

Field rules:

- Leaf fields dispatch by type to the matching parser: `Located<String>`, `Located<i64>`, `Located<f64>`, `Located<bool>`, `Located<PathBuf>`, `Vec<Located<String>>`, and `Option<Located<Vec<Located<String>>>>`.
- `Option<...>` makes a field optional.
  A non-optional field with no default reports `missing field` when absent.
- **`#[confval(nested)]`** delegates to the field type's own `FromFields` impl.
  Works for single structs, optional structs, and `Vec` of structs (repeated blocks).
- **`#[confval(default)]`** and **`#[confval(default = expr)]`** fill an absent field with a detached default instead of reporting it missing.
  A bare `#[confval(default)]` also applies to a non-optional nested field (`Located<S>` with `#[confval(nested, default)]`), filling an omitted block with `S::default()`.
  `default = expr` is leaf-only.
- Unknown fields in the input are reported as errors.

Tagged unions (a block whose shape depends on a discriminator field like `mode` or `type`) are hand-written `FromFields` impls.
The derive only handles plain structs.

## #[derive(confval::Config)]

Generates the `Lower` impl that converts a validated spec into a runtime config:

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
  This is the leaf-default's structural counterpart, and replaces hand-written `*_or_default` lowering functions.
- **`#[confval(lower(from = field, with = fn))]`**: explicit conversion through a function `fn(&SpecField, &mut Report) -> Option<Target>`.
  All narrowing (`i64` to `u16`, string to enum, string to `IpNet`) goes through these functions.
  `from` also accepts a tuple `(a, b)` when one config field derives from several spec fields.
- **`#[confval(spec_only(field, ...))]`** at the struct level names spec fields that intentionally have no runtime counterpart.

The generated impl destructures the spec exhaustively with no rest pattern.
Adding a field to either struct without accounting for it on the other side is a compile error, which keeps spec and config in lockstep.

## Narrowing helpers

`confval::pipeline::narrow` provides ready-made `with` functions.
For integer width changes: `i64_to_u16`, `i64_to_u32`, `i64_to_u64`, `i64_to_usize`, and `opt_` variants for optional fields.
They narrow with `try_from` rather than `as`: a value that does not fit is reported at its span and lowering fails, so a missing range rule surfaces as a located error instead of a silent truncation.
`i64_secs_to_duration` (and `opt_i64_secs_to_duration`) route a seconds count through the same checked narrow into a `Duration`, rejecting a negative value rather than wrapping it.
`i64_to_f64` widens to `f64` for the ratio and rate fields where an `as` cast cannot be named in a `with` attribute.

```rust
use confval::pipeline::narrow;

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
}
```
