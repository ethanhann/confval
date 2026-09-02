---
sidebar_position: 3
---

# Lowering

Once a spec is validated, lowering converts it into a config type.
A config type is the runtime form your program uses.
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

### Field rules

- **No attribute**: the field auto-maps via the `LowerAuto` trait, which strips `Located` wrappers without narrowing.
  - `Located<T>` becomes `T`
  - `Option<Located<T>>` becomes `Option<T>`
  - `Vec<Located<T>>` becomes `Vec<T>`
  - `Located<Vec<Located<T>>>` becomes `Vec<T>`
  - The optional variant of the last shape works as well.

- **`#[confval(nested)]`**: the field type implements `Lower` itself.
  Works for single, `Option`, and `Vec` shapes.

- **`#[confval(nested, default)]`**: a non-optional config field lowered from an `Option<Located<S>>` spec field.
  When the source omits the block, `S::default()` is lowered in its place.
  The runtime field is always populated while the spec stays source-faithful (an absent block stays `None`).
  This attribute also exists on the spec side, where it fills the omitted block during parsing instead of at lowering.
  See [Optional fields and defaults](./parsing.md#optional-fields-and-defaults) for the difference.

- **`#[confval(lower(from = field, with = fn))]`**: explicit conversion through a function `fn(&SpecField, &mut Report) -> Option<Target>`.
  All narrowing (`i64` to `u16`, string to enum, string to `IpNet`) goes through these functions.
  `from` also accepts a tuple `(a, b)` when one config field derives from several spec fields.

- **`#[confval(spec_only(field, ...))]`** at the struct level names spec fields that intentionally have no runtime counterpart.

The generated impl destructures the spec exhaustively with no rest pattern.
Adding a field to either struct without accounting for it on the other side is a compile error.
The two structs stay in agreement.

## Narrowing helpers

`confval::pipeline::narrow` provides ready-made `with` functions.

### Integer helpers

| Helper | Input | Output | Notes |
|--------|-------|--------|-------|
| `i64_to_u16` | `&Located<i64>` | `Option<u16>` | Narrows with `try_from`. Reports at span on overflow. |
| `i64_to_u32` | `&Located<i64>` | `Option<u32>` | Same behavior. |
| `i64_to_u64` | `&Located<i64>` | `Option<u64>` | Same behavior. |
| `i64_to_usize` | `&Located<i64>` | `Option<usize>` | Same behavior. |
| `opt_i64_to_u16` | `&Option<Located<i64>>` | `Option<Option<u16>>` | Optional variant of `i64_to_u16`. |
| `opt_i64_to_u32` | `&Option<Located<i64>>` | `Option<Option<u32>>` | Optional variant of `i64_to_u32`. |
| `opt_i64_to_u64` | `&Option<Located<i64>>` | `Option<Option<u64>>` | Optional variant of `i64_to_u64`. |
| `opt_i64_to_usize` | `&Option<Located<i64>>` | `Option<Option<usize>>` | Optional variant of `i64_to_usize`. |
| `i64_secs_to_duration` | `&Located<i64>` | `Option<Duration>` | Checked narrow to `u64`, then `Duration::from_secs`. Rejects negative values. |
| `opt_i64_secs_to_duration` | `&Option<Located<i64>>` | `Option<Option<Duration>>` | Optional variant. |
| `i64_to_f64` | `&Located<i64>` | `Option<f64>` | Widens to `f64` for ratio and rate fields. |

All integer helpers narrow with `try_from` rather than `as`.
A value that does not fit is reported at its span and lowering fails.
A missing range rule is reported as a located error instead of silently truncating the value.

### Keyword helpers

| Helper | Input | Output | Notes |
|--------|-------|--------|-------|
| `keyword::<T>` | `&Located<String>` | `Option<T>` | Lowers a validated keyword string into the enum `T`. Uses `TryFrom<&str>`. |
| `keyword_list::<T>` | `&Vec<Located<String>>` | `Option<Vec<T>>` | Lowers each element. Reports every failure before returning. |
| `opt_keyword_list::<T>` | `&Option<Located<Vec<Located<String>>>>` | `Option<Option<Vec<T>>>` | Unwraps the optional wrapper. Returns `Some(None)` for an absent field. |

Name `keyword::<T>` with a turbofish so the derive knows which enum to parse into.
The field was validated against the same set the `TryFrom` accepts.
The conversion does not fail in a running pipeline.
The helper reports at the value's span in two cases: a `keyword_set()` check left out of the `Validate` impl, and a handwritten keyword set that disagrees with its enum.
`keyword_enum!` prevents the second case.

`keyword_list::<T>` reports every bad element before it returns.
An operator sees all of them in one run.
A single bad element leaves the whole field unlowered.

Record the set on the field with `#[confval(keywords = ...)]`.
A bad element is then reported at its own span during validation rather than through the lowering helper's defensive branch.
Handwritten specs reach the same check through [`check_each`](./validation.md#keywordset).

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
