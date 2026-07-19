---
sidebar_position: 3
---

# The format layer

`confval::format` is where text becomes data.
It has two halves: a format-neutral field model that everything downstream works against, and one frontend per syntax that produces it.

## The neutral field model

A frontend parses its own syntax tree and lowers it into the owned types in `confval::format::field` (re-exported at `confval::format`).
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

## FromFields

Types that parse from configuration implement the format-neutral trait:

```rust
pub trait FromFields: Sized {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self>;
}
```

Returning `None` means the value could not be constructed.
The reason is already in the report.
Implementations should parse every field they can before returning, so one bad field does not hide the others.

## Parsing helpers

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
All of these are format-neutral and shared by every frontend.

## Frontends

A frontend's only job is text-to-`Fields`.
Each is a thin module behind its own feature:

| Entry point                         | Feature | Parser      |
|-------------------------------------|---------|-------------|
| `confval::format::hcl::parse_hcl`   | `hcl`   | `hcl-edit`  |
| `confval::format::toml::parse_toml` | `toml`  | `toml_edit` |

Both have the signature `fn(&SourceMap, SourceId, &mut Report) -> Option<T> where T: FromFields`, and both produce the same neutral `Fields`, so the leaf parsers, the derive output, and every hand-written `FromFields` impl work against either format unchanged.

The two HCL spellings normalize the same way.
A block (`bind { port = 8080 }`) becomes a `FieldKind::Block`, and an attribute-with-object (`bind = { port = 8080 }`) becomes a `FieldKind::Value` holding a `Map`.
TOML maps analogously: a `[table]` is a block, an inline `{ ... }` is a map value, and an array of tables (`[[x]]`) becomes one field whose value is a sequence of maps, so a `Vec` of nested structs lowers from it exactly as it would from an HCL array of objects.

:::note
`hcl-edit` rejects duplicate attribute keys at parse time, so duplicate attributes surface as syntax errors.
Duplicate blocks parse fine and are reported by `parse_single_struct` with a related span pointing at the first occurrence.
:::
