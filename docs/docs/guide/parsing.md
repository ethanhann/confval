---
sidebar_position: 1
---

# Parsing

Parsing turns a configuration file into a spec type.
A spec type is a plain Rust struct whose fields are the settings you expect.

Spec is short for "specification."
The collection of spec types is the specification for an application's operator-facing configuration surface.

Parsing checks structure only.
It determines whether each field is present and has the right type.
What the values mean is left to [validation](./validation.md).

## Located values

Every field on a spec is wrapped in a `Located<T>`.

```rust
pub struct Located<T> {
    pub value: T,
    pub span: Span,
}
```

A `Located<T>` contains a span.
A span is a byte range in the configuration file.
It provides per-field provenance, which is a fancy way of saying the span allows the spec to know what line in the
configuration file the field came from.  
It is what lets a later error point at the exact line and column a value came from.
`Span` and the `SourceMap` that resolves it are covered under [Diagnostics](./diagnostics.md#spans-and-source).

A few behaviors are worth knowing:

- **Value-only equality.**
  `PartialEq`, `Eq`, and `Hash` ignore the span, so two configs with the same values compare equal regardless of
  formatting.
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

### Field types

A field's type tells the parser how to read it.
These are the types you can use:

- **Scalars**: `Located<String>`, `Located<i64>`, `Located<f64>`, `Located<bool>`, and `Located<PathBuf>`.
- **Lists of strings**: `Vec<Located<String>>`, or `Option<Located<Vec<Located<String>>>>` for an optional list.
- **Nested structs**: another `Spec` type marked with `#[confval(nested)]`, described below.

### Optional fields and defaults

Every field is required by default.
Leave a required field out of the file and the parser reports a `missing field` error against the block it belongs to.

Two attributes make a field optional:

- `Option<...>` on the type turns an absent field into `None`.
- `#[confval(default = expr)]` fills an absent field with `expr` instead of reporting it.
  For example, `#[confval(default = 30)]` gives the field the value `30` when the file leaves it out.

`default = expr` is for scalar fields.
For a nested struct, combine `#[confval(nested, default)]` with no expression, and an omitted block is filled with that
struct's `Default`.

### Nested structs

`#[confval(nested)]` tells the parser to read a field with its own `Spec` type instead of as a scalar.
It works three ways:

- a single struct, `Located<T>`
- an optional struct, `Option<Located<T>>`
- a list of structs, `Vec<Located<T>>`, which reads a block that may repeat

### Unknown fields

A setting in a configuration file that does not exist in the Rust struct will be interpreted as a parsing error.
There is no lenient mode that ignores extra settings/keys.

Stricter is better in general with configuration file structure, but this is particularly useful for LLM-edited
configuration files as LLMs tend to invent settings that do not exist.

### What the derive does not handle

The derive only handles plain structs.
The one common case it cannot express is an enum (e.g., `mode` or `type` fields with a discrete set of values).

Write those by hand, described in [Writing parsers by hand](#writing-parsers-by-hand).

## Parsing a file

To parse, call the frontend for the format you enabled with the appropriate feature:

| Entry point                         | Feature | Backed by   |
|-------------------------------------|---------|-------------|
| `confval::format::hcl::parse_hcl`   | `hcl`   | `hcl-edit`  |
| `confval::format::toml::parse_toml` | `toml`  | `toml_edit` |

Each takes a `SourceMap`, a `SourceId`, and a `&mut Report`, and returns your spec as an `Option`.

```rust
let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);
```

The result is the same whichever format you read, so validation and lowering never depend on which frontend ran.

Both HCL spellings of a nested block parse into the same thing.
A block, `bind { port = 8080 }`, and an attribute set to an object, `bind = { port = 8080 }`, are equivalent.
TOML lines up with this: a `[table]` is a block, an inline `{ ... }` is an object, and an array of tables (`[[x]]`) is a
repeating block, so a `Vec` of nested structs reads from it the same way it reads from an HCL list of objects.

:::note
`hcl-edit` rejects duplicate attribute keys while parsing, so a repeated attribute is a syntax error.
A repeated block parses, and confval reports it with a related span pointing at the first occurrence.
:::

## Writing parsers by hand

A little background before getting into the specifics of handwritten parsers...

Most specs never need this.
The `Spec` derive covers plain structs, which is nearly everything given the confval [pipeline contract](../pipeline.md).

The confval approach, as much as possible, reduces a spec's fields to primitive types:

1. A spec should have the most broadly typed form of the field.
2. After validation, the lowering process narrows the value to a more specific type.

For a tagged union (e.g., a discrete set of values like "red", "green", "blue") the spec would contain a string.
The validation stage uses [KeywordSet](./validation.md#keywordset) to validate the discrete list.
Then, when the lowering stage converts the spec to a runtime type, the string is converted to an enum. 

But what if, for some reason, you do not want to do this?

This is where handwritten parsers are useful. 

For example, you could write a parser for a spec struct with an enum (defying confval conventions). 

This can be done by manually implementing confval's `FromFields` trait. 
A parser is an implementation of this trait.
It is the same the trait the derive automatically generates.

```rust
pub trait FromFields: Sized {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self>;
}
```

Return `None` when the value cannot be built.
The reason is already in the report.
Parse every field you can before returning, so one bad field does not hide the others.

### The field model

A handwritten parser reads `Fields`, confval's format-neutral view of one level of structure.
A frontend builds it, and from there nothing knows which format the text was.

- **`Fields`** is one level: the named entries of a body, table, or inline object, plus the span a missing-field error
  points at.
- **`Field`** is one entry: its name, the span of the name, the span of the whole entry, and a `FieldKind`.
- **`FieldKind`** is either `Value` for an attribute (`name = value`) or `Block` for a block (`name { ... }` in HCL,
  `[name]` in TOML).
  The split lets a diagnostic say "found block" rather than "found object".
- **`Value`** is a span plus a `ValueKind`: a `Scalar`, a `Seq` (a list), a `Map` (nested fields), or `Other`.
- **`Scalar`** is `String`, `Int(i64)`, `Float(f64)`, or `Bool`.
  Integers and floats stay distinct so a format that separates them, like TOML's `1` and `1.0`, round-trips faithfully.
- **`Other(label)`** is a value that exists in the file but sits outside the model, such as an HCL template or a TOML
  datetime.
  It always surfaces as a plain type mismatch named by the label, for example `expected string, found datetime`.

### Helpers

confval ships helpers so a handwritten parser reports exactly like the derive does.

Leaf parsers turn one `Field` into a `Located` value, reporting a typed error on mismatch:

- `parse_string_field`, `parse_int_field` (i64), `parse_float_field`, `parse_bool_field`
- `parse_string_list_field` for arrays of strings

Structural parsers recurse through `FromFields`:

- `parse_struct_field`: one nested struct from a block or a map value
- `parse_single_struct`: like `parse_struct_field`, but reports duplicates when the field appears more than once
- `parse_struct_list_field`: repeated blocks or a sequence of maps, collected into a `Vec`

Reporting helpers keep messages uniform: `report_unknown_field`, `report_missing_field`, `report_duplicate_field`.
Unknown fields are always errors.
There is no lenient mode.
