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

## Concept Overview

This is a high-level look at parsing.
The sections below cover each part in more detail.

You define a spec as a struct, then parse a file into it with the frontend for the format you enabled.

```rust
use confval::prelude::*;

#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
}

let text = r#"hostname = "127.0.0.1"
port = 8080
"#;

let mut sources = SourceMap::new();
let mut report = Report::new();
let id = sources.add("server.hcl", text);

let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);
```

Every field is wrapped in a `Located<T>`, which pairs the value with its source span, covered next.
The parse checks structure only.
It reports a missing field, a wrong type, or an unknown field, each at its span, and leaves what the values mean to [validation](./validation.md).

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
It gives each field its provenance.
The span records where the value came from, so a later error can point at the exact line and column.
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

Two things make a field optional.

- `Option<...>` on the type turns an absent field into `None`.
- `#[confval(default)]` or `#[confval(default = expr)]` fills an absent field instead of reporting it.

A bare `#[confval(default)]` uses the field type's `Default`.
The `default = expr` form uses `expr` instead, so `#[confval(default = 30)]` gives the field the value `30` when the
file leaves it out.
A filled-in value carries a detached span, because no source text stands behind it.

Which form a field accepts depends on its shape.

| Field shape                                    | `#[confval(default)]` | `#[confval(default = expr)]` |
|------------------------------------------------|-----------------------|------------------------------|
| `Located<T>`                                   | `T::default()`        | `expr`                       |
| `Option<Located<T>>`                           | `Some(T::default())`  | `Some(expr)`                 |
| `Vec<Located<String>>`                         | empty list            | compile error                |
| `Located<S>` with `#[confval(nested)]`         | `S::default()`        | compile error                |
| `Option<Located<S>>` with `#[confval(nested)]` | compile error         | compile error                |
| `Vec<Located<S>>` with `#[confval(nested)]`    | compile error         | compile error                |
| `Option<Located<Vec<Located<String>>>>`        | compile error         | compile error                |

Three rows are worth calling out.

Combining `Option` with a default means the field is never `None` for an absent value.
The default fills it in.
Leave the default off when you need the `Option` to report what the source omitted.

An optional nested block rejects a default because an absent block already yields `None`.
A nested list rejects one because a list of blocks is zero-or-more already.

A string list accepts only the bare form, where the default is the empty list.
There is no `default = expr` for a list.

:::caution
The spelling `#[confval(nested, default)]` also exists on the config side, where it means something different.
On a spec it fills the omitted block during parsing, so the spec itself holds the default.
On a config it leaves the spec field `None` and lowers `S::default()` in its place, so the spec stays faithful to the
source and only the runtime value is filled in.
The two are independent, and one setting can use either, both, or neither.
See [Lowering](./lowering.md#defining-a-config).
:::

### Deriving `Default` from the attribute defaults

The attribute default fills a field the file omits.
When the whole block is omitted, the config side supplies it through `#[confval(nested, default)]`, which lowers `S::default()`, so the spec type needs a `Default` impl.
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

This resembles `#[derive(Default)]`.
The difference is where the values come from.
The standard derive fills each field with `T::default()`, so a `Located<String>` becomes empty and a `Located<i64>` becomes zero.
`#[confval(derive_default)]` fills each field from its declared `#[confval(default)]` instead.
It refuses a field that declares no default rather than inventing a value.

Use `#[confval(derive_default)]` rather than `#[derive(Default)]` on a spec.
The standard derive fills an undeclared field with `T::default()` without reporting it, so the value for an absent block and the value for a field the source omits can drift apart.
`#[confval(derive_default)]` keeps those two values the same.

The value it generates for a field is the value the parser fills when that field is absent.
A field the parser would report as missing has no value to derive, so it is a compile error.
A non-optional `Located<T>` or `Located<S>` with no default, and a `Vec<Located<String>>` with no default, each need a `#[confval(default)]` or a handwritten `impl Default`.
An `Option` field and a nested list default on their own, because the parser already fills them when they are absent.

The attribute is opt-in and additive, so a type that keeps its handwritten `impl Default` is unaffected.

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
It cannot express an enum.

That is rarely a problem, because a field with a discrete set of values is a `Located<String>` in a spec by convention
rather than an enum.
[Writing parsers by hand](#writing-parsers-by-hand) covers that convention, along with the shapes that do need a
handwritten parser.

## Parsing a file

To parse, call the frontend for the format you enabled with the appropriate feature:

| Entry point                         | Feature | Backed by   |
|-------------------------------------|---------|-------------|
| `confval::format::hcl::parse_hcl`   | `hcl`   | `hcl-edit`  |
| `confval::format::toml::parse_toml` | `toml`  | `toml_edit` |
| `confval::format::kdl::parse_kdl`   | `kdl`   | `kdl`       |

Each takes a `SourceMap`, a `SourceId`, and a `&mut Report`, and returns your spec as an `Option`.

```rust
let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);
```

The result is the same whichever format you read, so validation and lowering never depend on which frontend ran.

Both HCL spellings of a nested block parse into the same thing.
A block, `bind { port = 8080 }`, and an attribute set to an object, `bind = { port = 8080 }`, are equivalent.
TOML lines up with this: a `[table]` is a block, an inline `{ ... }` is an object, and an array of tables (`[[x]]`) is a
repeating block, so a `Vec` of nested structs reads from it the same way it reads from an HCL list of objects.

KDL spells the same shapes with nodes.
It parses with the KDL 2.0 grammar alone.
A children block, `bind { port 8080 }`, and properties on one node, `bind port=8080`, are the same nested structure.
A list is repeated arguments on one node, `allow "a" "b"`, or repeated same-named nodes, and a bare node is an empty
list, the only spelling KDL has for one.

For example, this document fills the same spec the HCL and TOML snippets above fill:

```kdl
hostname "127.0.0.1"
port 8080
allow "10.0.0.0/8" "192.168.0.0/16"

bind {
  port 8080
}
```

```rust
let spec: Option<ServerSpec> = confval::format::kdl::parse_kdl(&sources, id, &mut report);
```
A repeated node is a list when the field is a list and a duplicate error when it is not.
An argument on a node that also has properties or children is an error, because the model has no block labels.
A bare node where a single value is expected reports `expected string, found array`, because the bare spelling means an
empty list.

A list field also accepts a single string as a one-element list, in every format.
KDL forces the question, because it has no array literal and spells a one-element list as a single value, and the
answer is uniform so the same configuration means the same thing whichever frontend read it.

:::note
`hcl-edit` rejects duplicate attribute keys while parsing, so a repeated attribute is a syntax error, and TOML rejects
a duplicate key the same way.
A repeated block parses, and confval reports it with a related span pointing at the first occurrence.
A repeated KDL value node reaches the same rule: a list field accumulates the occurrences, and a single-value field
reports the repeat with the related span.
:::

## Writing parsers by hand

Most specs never need this.
The `Spec` derive covers plain structs, which is nearly everything given the
confval [pipeline contract](../pipeline.md).

By convention, confval reduces a spec's fields to primitive types wherever it can.
A spec holds the most broadly typed form of a field.
Lowering narrows that value to a more specific type once validation has run.

A discrete set of values follows the same pattern rather than becoming an enum in the spec.
Take a `mode` field that accepts "red", "green", or "blue" as an example.
The spec holds a `Located<String>`.
Validation checks it against a [KeywordSet](./validation.md#keywordset).
Lowering converts the string to an enum.

Handwritten parsers cover the shapes that pattern cannot express.
The clearest case is a block whose remaining fields depend on a discriminator, where the parser reads the discriminator
first and dispatches on it.
The same mechanism lets you put an enum directly in a spec.
That compiles and parses correctly.
It also abandons the convention above, which is why it is not the recommended shape.

A parser is an implementation of confval's `FromFields` trait.
It is the same trait the derive generates.

```rust
pub trait FromFields: Sized {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self>;
}
```

A `Fields` built by `to_template` can also hold commented-out entries, fields whose `commented` flag is set.
Such a field reads as absent.
`Fields::get` and `Fields::has` skip one for you.
If you iterate with `Fields::iter`, check the flag and skip the field the way the generated walk does.

An implementation parses every field before deciding what to return.
Parsing all of them first keeps one bad field from hiding the problems in the others.

Report each problem as you find it.
Then return `None` when a field failed and the value cannot be built.
The `None` itself carries no reason.
Whatever explains the failure must already be in the report.

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
- **`Scalar`** is `String`, `Int(i64)`, `Float(f64)`, `Bool`, or `Unparsed`.
  Integers and floats stay distinct so a format that separates them, like TOML's `1` and `1.0`, round-trips faithfully.
- **`Unparsed(String)`** is the raw text of a value from a source that only carries strings, such as an environment
  variable or a command line flag.
  The leaf parsers coerce it to the type they expect, so the field's declared type decides what `"8080"` becomes.
  No file frontend produces it.
  A quoted string in a file stays a `String`.
- **`Other(label)`** is a value that exists in the file but falls outside the model, such as an HCL template or a TOML
  datetime.
  It always surfaces as a plain type mismatch named by the label, for example `expected string, found datetime`.

### Helpers

confval ships helpers so a handwritten parser reports exactly like the derive does.

Leaf parsers turn one `Field` into a `Located` value, reporting a typed error on mismatch:

- `parse_string_field`, `parse_int_field` (i64), `parse_float_field`, `parse_bool_field`, `parse_path_field`
- `parse_string_list_field` for arrays of strings

Structural parsers recurse through `FromFields`:

- `parse_struct_field`: one nested struct from a block or a map value
- `parse_single_struct`: like `parse_struct_field`, but reports duplicates when the field appears more than once
- `parse_struct_list_field`: repeated blocks or a sequence of maps, collected into a `Vec`

Occurrence helpers guard a field that may appear only once:

- `first_occurrence`: records the first occurrence of a leaf field and reports a later one as a duplicate
- `parse_single_struct`: the same guard around a nested block

The derive wraps every leaf arm it generates in `first_occurrence`, so a derived spec reports a repeated field.
A handwritten parser that assigns its slot directly takes the last value instead, with no diagnostic.
KDL delivers a repeated scalar as separate fields, so the difference shows up in a real document.

Reporting helpers keep messages uniform: `report_unknown_field`, `report_missing_field`, `report_duplicate_field`.
Unknown fields are always errors.
There is no lenient mode.

The `handwritten` example calls these helpers against a tagged enum and a handwritten root.
The test at `crates/confval/tests/handwritten_parity.rs` writes one spec both ways and asserts that the two write walks
render the same text and agree on every span.

## Writing emitters by hand

A type with a handwritten `FromFields` needs a handwritten `ToFields` too, because the derive generates one only for the
types it parses.
`ToFields` has two required walks.
`to_fields` emits every field with its span detached, which is the populated view and the template.
`to_source_fields` emits only the fields the source set, keeps their spans, and recurses into children with
`to_source_fields` rather than `to_fields`.
That output is the [source view](./representations.md).

Writing both by hand means writing the field list twice and reproducing that difference on every line.
`FieldsBuilder` takes the walk as a parameter instead, so you list the fields once:

```rust
use confval::format::{Fields, FieldsBuilder, ToFields, Walk};

impl Server {
    fn build(&self, walk: Walk) -> Fields {
        FieldsBuilder::new(walk)
            .leaf("hostname", &self.hostname)
            .leaf_opt("port", self.port.as_ref())
            .string_list("tags", &self.tags)
            .block("limits", &self.limits)
            .finish()
    }
}

impl ToFields for Server {
    fn to_fields(&self) -> Fields {
        self.build(Walk::Populated)
    }

    fn to_source_fields(&self) -> Fields {
        self.build(Walk::Source)
    }
}
```

Each method takes the `Located` rather than the value inside it, so the builder has the span each walk needs.

| Method | `Walk::Populated` | `Walk::Source` |
|--------|-------------------|----------------|
| `leaf`, `leaf_opt` | emits detached | emits with its span, or omits a detached one |
| `string_list` | emits every element detached | emits the elements that carry a span, or omits the field |
| `string_list_opt` | emits detached when present | emits when the wrapper carries a span, elements included |
| `block`, `block_opt`, `block_list` | recurses with `to_fields` | recurses with `to_source_fields` when the block carries a span |
| `block_opt_default` | fills an absent block from `S::default()` | omits an absent block |
| `literal_string` | emits detached | emits detached |

`block_opt_default` is the counterpart of `#[confval(nested, default)]`, whose populated walk shows the values the
program will run with even for a block the operator never wrote.
Use `block_opt` for an optional block with no default, which both walks omit when it is absent.

`leaf` accepts the same types a derived spec field accepts, through the sealed `Leaf` trait: `String`, `i64`, `f64`,
`bool`, and `PathBuf`.
No crate outside confval implements it, so the list can grow in a minor release.
A path emits as a string, the one lossy conversion, matching what the derive generates.

`literal_string` is for a field your impl supplies rather than reads.
That field is the discriminator on a tagged enum:

```rust
match self {
    TlsSpec::Manual { cert, key } => builder
        .literal_string("mode", "manual")
        .leaf("cert", cert)
        .leaf("key", key),
    TlsSpec::Acme { domains } => builder
        .literal_string("mode", "acme")
        .string_list("domains", domains),
};
```

Both walks emit it, because a source view that dropped the tag would not reparse.

The builder does not cover every shape a spec can hold, a string-keyed map among them.
Build such a field directly with `Field::detached_value` or `Field::detached_block`.
Locate it with `at` when it carries a span, then `push` it into the builder where it belongs:

```rust
let field = Field::detached_value(name, value).at(span);
builder.push(field);
```

The walk does not reach a pushed field.
You decide what it carries.
On a source walk that includes deciding whether the source set it.
The builder still shapes every other field of the type.

`at` sets the field's span and its source.
An attribute's value takes the same span.
A block's nested level keeps its own source and enclosing span.
A sequence's elements keep the spans they were built with.

A handwritten spec type also implements `Validate` and `ValidateNested`.
`Validate` holds its rules.
`ValidateNested` holds the descent into its children, the traversal the derive would have written from the struct
definition.
The `Self: ValidateNested` bound on `validate_all` makes omitting the traversal a compile error rather than a silently
skipped subtree.
A type in a required nested slot also needs `Default`, because the generated parser fills an absent block with it before
reporting the block missing.

`to_template` defaults to `to_fields` for a handwritten impl.
That fallback recurses with `to_fields`, so doc comments stop at the first handwritten node and never reach anything
below it.
A derived block nested under a handwritten one renders without its comments.
The `handwritten` example prints both sides of that boundary.
