---
sidebar_position: 2
---

# Parsing

Parsing turns a configuration file into a spec type.
A spec type is a plain Rust struct whose fields are the settings you expect.
Parsing checks structure only: whether each field is present and has the right type.
What the values mean is left to [validation](./validation.md).

## Located values

Every field on a spec is wrapped in a `Located<T>`, which pairs the parsed value with the `span` it came from.

```rust
pub struct Located<T> {
    pub value: T,
    pub span: Span,
}
```

The span is a byte range in the source file.
It is what lets a later error point at the exact line and column a value came from.
`Span` and the `SourceMap` that resolves it are covered under [Diagnostics](./diagnostics.md#spans-and-source).

A few behaviors are worth knowing:

- **Value-only equality.**
  `PartialEq`, `Eq`, and `Hash` ignore the span, so two configs with the same values compare equal regardless of formatting.
- **`Deref` to `T`.**
  Method calls pass through to the inner value.
- **`Located::detached(value)`** produces a value with a sentinel span.
  Use it to build a spec in code (tests, builders, generated templates) with no source file behind it.
- **`Default`** is `detached(T::default())`.
- With the `serde` feature, `Located<T>` serializes transparently as `T` and deserializes detached.

## Defining a spec

`#[derive(confval::Spec)]` writes the parser for a struct.
Parsing is purely structural, so the macro never embeds semantic rules.

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

## The field model

Under the derive, parsing runs against a format-neutral field model.
A frontend parses its own syntax and lowers it into the owned types in `confval::format::field` (re-exported at `confval::format`).
Once a `Fields` exists, nothing downstream knows which format it came from.

- **`Fields`** is one structural level: the named entries of a body, a table, or an inline object, plus the enclosing span a missing-field error points at.
- **`Field`** is one entry: its name, the span of the name, the span of the whole entry, and a `FieldKind`.
- **`FieldKind`** is `Value(Value)` for an attribute (`name = value`) or `Block(Fields)` for a block (`name { ... }` in HCL, `[name]` in TOML).
  The split exists so diagnostics can say "found block" rather than "found object".
- **`Value`** is a span plus a `ValueKind`: `Scalar(Scalar)`, `Seq(Vec<Value>)`, `Map(Fields)`, or `Other(&'static str)`.
- **`Scalar`** is `String`, `Int(i64)`, `Float(f64)`, or `Bool`.
  Integers and floats are kept distinct so a format that separates them syntactically (TOML's `1` vs `1.0`) round-trips faithfully.
- **`Other(label)`** is a value present in source but outside the model (an HCL template or `null`, a TOML datetime).
  No leaf parser matches it, so it always surfaces as an ordinary type mismatch whose noun is the label, for example `expected string, found datetime`.

## Hand-written parsers

Most specs use the derive.
For shapes it does not cover, such as tagged unions, implement the trait directly.

```rust
pub trait FromFields: Sized {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self>;
}
```

Returning `None` means the value could not be constructed, and the reason is already in the report.
Parse every field you can before returning, so one bad field does not hide the others.

confval ships helpers so a hand-written impl reports the same way the derive does.

Leaf parsers convert one `Field` into a `Located` value, reporting a typed error on mismatch:

- `parse_string_field`, `parse_int_field` (i64), `parse_float_field`, `parse_bool_field`
- `parse_string_list_field` for arrays of strings

Structural parsers recurse through `FromFields`:

- `parse_struct_field`: one nested struct from a block or a map value
- `parse_single_struct`: like `parse_struct_field`, but reports duplicates when the field appears more than once
- `parse_struct_list_field`: repeated blocks or a sequence of maps, collected into a `Vec`

Reporting helpers keep messages uniform: `report_unknown_field`, `report_missing_field`, `report_duplicate_field`.
Unknown fields are always errors.
There is no lenient mode.

## Frontends: HCL and TOML

A frontend's only job is to turn text into `Fields`.
Each is a thin module behind its own feature:

| Entry point                         | Feature | Parser      |
|-------------------------------------|---------|-------------|
| `confval::format::hcl::parse_hcl`   | `hcl`   | `hcl-edit`  |
| `confval::format::toml::parse_toml` | `toml`  | `toml_edit` |

Both have the signature `fn(&SourceMap, SourceId, &mut Report) -> Option<T> where T: FromFields`, and both produce the same neutral `Fields`.
So the leaf parsers, the derive output, and every hand-written `FromFields` impl work against either format unchanged.

The two HCL spellings normalize the same way.
A block (`bind { port = 8080 }`) becomes a `FieldKind::Block`, and an attribute-with-object (`bind = { port = 8080 }`) becomes a `FieldKind::Value` holding a `Map`.
TOML maps analogously: a `[table]` is a block, an inline `{ ... }` is a map value, and an array of tables (`[[x]]`) becomes one field whose value is a sequence of maps, so a `Vec` of nested structs lowers from it exactly as it would from an HCL array of objects.

:::note
`hcl-edit` rejects duplicate attribute keys at parse time, so duplicate attributes surface as syntax errors.
Duplicate blocks parse fine and are reported by `parse_single_struct` with a related span pointing at the first occurrence.
:::
