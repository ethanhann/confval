---
sidebar_position: 1
---

# Parsing

When you define a configuration for your application, the first step is to parse a file into a spec type.
A spec type is a plain Rust struct whose fields are the settings you expect.

"Spec" is short for "specification."
The collection of spec types is the specification for your application's operator-facing configuration surface.

Parsing checks structure only.
It determines whether each field is present and has the right type.
What the values mean is left to [validation](./validation.md).

## A first parse

Define a spec as a struct, then parse a file into it with the frontend for the format you enabled.

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

Every field is wrapped in a `Located<T>`, which pairs the value with its source span.
The parse reports a missing field, a wrong type, or an unknown field, each at its span.
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
It records where the value came from, so a later error can point at the line and column.
`Span` and the `SourceMap` that resolves it are covered under [Diagnostics](./diagnostics.md#spans-and-source).

| Behavior | Detail |
|----------|--------|
| Value-only equality | `PartialEq`, `Eq`, and `Hash` ignore the span. Two configs with the same values compare equal regardless of formatting. |
| `Deref` to `T` | Method calls pass through to the inner value. |
| `Located::detached(value)` | Produces a value with a sentinel span. Use it to build a spec in code (tests, builders, generated templates) with no source file behind it. |
| `Default` | `detached(T::default())`. |
| serde | With the `serde` feature, `Located<T>` serializes transparently as `T` and deserializes detached. |

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

- **Scalars**: `Located<String>`, `Located<i64>`, `Located<f64>`, `Located<bool>`, and `Located<PathBuf>`.
- **Lists of strings**: `Vec<Located<String>>`, or `Option<Located<Vec<Located<String>>>>` for an optional list.
- **Nested structs**: another `Spec` type marked with `#[confval(nested)]`, described below.
- **Maps**: an open-ended, string-keyed map, `BTreeMap<String, Located<String>>` marked with `#[confval(map)]`, described below.

### Optional fields and defaults

Every field is required by default.
Leave a required field out of the file and the parser reports a `missing field` error against the block it belongs to.

Two things make a field optional:

- `Option<...>` on the type turns an absent field into `None`.
- `#[confval(default)]` or `#[confval(default = expr)]` fills an absent field instead of reporting it.

A bare `#[confval(default)]` uses the field type's `Default`.
The `default = expr` form uses `expr` instead.
For example, `#[confval(default = 30)]` gives the field the value `30` when the file leaves it out.
A filled-in value has a detached span, because no source text stands behind it.

Which form a field accepts depends on its shape.

| Field shape                                    | `#[confval(default)]` | `#[confval(default = expr)]` |
|------------------------------------------------|-----------------------|------------------------------|
| `Located<T>`                                   | `T::default()`        | `expr`                       |
| `Option<Located<T>>`                           | `Some(T::default())`  | `Some(expr)`                 |
| `Vec<Located<String>>`                         | empty list            | compile error                |
| `BTreeMap<...>` with `#[confval(map)]`         | empty map             | compile error                |
| `Located<S>` with `#[confval(nested)]`         | `S::default()`        | compile error                |
| `Option<Located<S>>` with `#[confval(nested)]` | compile error         | compile error                |
| `Vec<Located<S>>` with `#[confval(nested)]`    | compile error         | compile error                |
| `Option<Located<Vec<Located<String>>>>`        | compile error         | compile error                |

Combining `Option` with a default means the field is never `None` for an absent value.
The default fills it in.
Leave the default off when you need the `Option` to report what the source omitted.

An optional nested block rejects a default because an absent block already yields `None`.
A nested list rejects one because a list of blocks is zero-or-more already.

A string list accepts only the bare form, where the default is the empty list.
There is no `default = expr` for a list.

:::caution
The attribute `#[confval(nested, default)]` also exists on the config side, where it means something different.
On a spec it fills the omitted block during parsing.
The spec itself holds the default.
On a config it leaves the spec field `None` and lowers `S::default()` in its place.
The spec stays faithful to the source and only the runtime value is filled in.
The two are independent.
One setting can use either, both, or neither.
See [Lowering](./lowering.md#defining-a-config).
:::

### Deriving `Default` from the attribute defaults

The attribute default fills a field the file omits.
When the whole block is omitted, the config side supplies it through `#[confval(nested, default)]`, which lowers `S::default()`.
The spec type therefore needs a `Default` impl.
Writing that impl by hand repeats the attribute defaults, and nothing keeps the two in agreement.

`#[confval(derive_default)]` on the struct generates the `Default` impl from the attribute defaults.
Each default is declared once.

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

The standard `#[derive(Default)]` fills each field with `T::default()`.
A `Located<String>` becomes empty and a `Located<i64>` becomes zero.
`#[confval(derive_default)]` fills each field from its declared `#[confval(default)]` instead.
It refuses a field that declares no default rather than inventing a value.

Use `#[confval(derive_default)]` rather than `#[derive(Default)]` on a spec.
The standard derive fills an undeclared field with `T::default()` without reporting it.
The value for an absent block and the value for a field the source omits can then drift apart.
`#[confval(derive_default)]` keeps those two values the same.

The value it generates for a field is the value the parser fills when that field is absent.
A field the parser would report as missing has no value to derive, so it is a compile error.
A non-optional `Located<T>` or `Located<S>` with no default, and a `Vec<Located<String>>` with no default, each need a `#[confval(default)]` or a handwritten `impl Default`.
An `Option` field and a nested list default on their own, because the parser already fills them when they are absent.

The attribute is opt-in and additive.
A type that keeps its handwritten `impl Default` is unaffected.

### Nested structs

`#[confval(nested)]` tells the parser to read a field with its own `Spec` type instead of as a scalar.
It works three ways:

- a single struct, `Located<T>`
- an optional struct, `Option<Located<T>>`
- a list of structs, `Vec<Located<T>>`, which reads a block that may repeat

### Maps

`#[confval(map)]` reads a field as an open-ended, string-keyed map, `BTreeMap<String, Located<String>>`.
Use it for a setting whose keys are not known ahead of time, such as HTTP request headers or URL templates.

The keys are open, so the parser reports no unknown field inside the map.
A duplicate key is an error.
Each value keeps its span, so a `Validate` impl can report a bad entry at the entry.
An operator writes the map as a block or as an inline map, and both read the same.

A bare `#[confval(map, default)]` reads an absent map as empty.
On the config side the map lowers to a plain `HashMap<String, String>` or `BTreeMap<String, String>` with no lowering function, because the two `LowerAuto` impls drop each value's span.

Only a string-keyed map with string values is supported.
A map of another value type, or a map of nested structs, needs a handwritten parser.

### Unknown fields

The parser reports a setting that the spec struct does not declare as an unknown field error.
There is no lenient mode that ignores extra settings.

An agent editing a configuration file can add a setting the spec does not declare.
The strict parse reports that setting instead of ignoring it.

### What the derive does not handle

The derive handles plain structs only.
It cannot express an enum.

That is rarely a problem, because a field with a discrete set of values is a `Located<String>` in a spec by convention rather than an enum.
[Writing parsers by hand](#writing-parsers-by-hand) covers that convention, along with the shapes that do need a handwritten parser.

## Parsing a file

To parse, call the frontend for the format you enabled:

| Entry point                         | Feature | Backed by       |
|-------------------------------------|---------|-----------------|
| `confval::format::hcl::parse_hcl`   | `hcl`   | `hcl-edit`      |
| `confval::format::toml::parse_toml` | `toml`  | `toml_edit`     |
| `confval::format::kdl::parse_kdl`   | `kdl`   | `kdl`           |
| `confval::format::json::parse_json` | `json`  | `jsonc-parser`  |
| `confval::format::yaml::parse_yaml` | `yaml`  | `saphyr-parser` |

Each takes a `SourceMap`, a `SourceId`, and a `&mut Report`, and returns your spec as an `Option`.

```rust
let spec: Option<ServerSpec> = confval::format::hcl::parse_hcl(&sources, id, &mut report);
```

The result is the same whichever format you read.
Validation and lowering never depend on which frontend ran.

### Nesting equivalences

Every format has more than one way to write a nested block.
Both forms parse into the same thing.

| Format | Block form | Object form |
|--------|-----------|-------------|
| HCL | `bind { port = 8080 }` | `bind = { port = 8080 }` |
| TOML | `[bind]` section | `bind = { port = 8080 }` inline table |
| KDL | `bind { port 8080 }` children block | `bind port=8080` properties on one node |
| JSON | (one form only) | `"bind": { "port": 8080 }` |
| YAML | block mapping | flow mapping `bind: { port: 8080 }` |

For example, these two documents fill the same spec:

```hcl
hostname = "127.0.0.1"
port = 8080
allow = ["10.0.0.0/8", "192.168.0.0/16"]

bind {
  port = 8080
}
```

```toml
hostname = "127.0.0.1"
port = 8080
allow = ["10.0.0.0/8", "192.168.0.0/16"]

[bind]
port = 8080
```

```rust
let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);
```

### Writing back to text

Write the model back out with the emitter for the format you want:

```rust
let text = confval::format::hcl::emit_hcl(&spec.to_fields())?;
```

The emitter for each format is named `emit_hcl`, `emit_toml`, `emit_kdl`, `emit_json`, or `emit_yaml`.

### Single-element list coercion

A list field also accepts a single string as a one-element list, in every format.
Every frontend applies the same rule.
One configuration reads the same way whichever frontend parsed it.

### Format-specific behavior

Each frontend handles edge cases according to the rules of its format.
The tables below collect the behaviors you are most likely to encounter.

#### HCL and TOML

| Behavior | Detail |
|----------|--------|
| Duplicate attribute | `hcl-edit` and `toml_edit` reject a duplicate attribute key during parsing. It is a syntax error. |
| Repeated block | Parses. confval reports it with a related span pointing at the first occurrence. |
| KDL-style label | HCL supports a native block label: `upstream "api" { ... }`. The label fills the child field the spec marks with `#[confval(label)]`. |

#### KDL

| Behavior | Detail |
|----------|--------|
| Grammar | KDL 2.0 only. |
| List | Repeated arguments on one node (`allow "a" "b"`), or repeated same-named nodes. A bare node is an empty list. |
| Repeated scalar node | A list when the field is a list. A `duplicate field` error when the field is single-valued. |
| Block label | A block node's first string argument is its native label (`upstream "api" { ... }`). It fills the child field marked with `#[confval(label)]`. |
| Non-string label | Reported. An argument past the first is also reported. |
| Bare node where scalar expected | Reports `expected string, found array`, because a bare node means an empty list. |

#### JSON

| Behavior | Detail |
|----------|--------|
| Strict mode | Strict JSON only. Comments, trailing commas, unquoted property names, missing commas, single-quoted strings, hexadecimal numbers, and numbers with a unary plus are each a syntax error. |
| Document root | Must be an object. Any other root reports `expected an object at the document root`. |
| Number classification | `1` is an integer. `1.0` and `1e3` are floats. |
| Oversized integer | An integer beyond `i64` range reports `expected integer, found oversized integer`. |
| Oversized number | A magnitude beyond `f64` (such as `1e999`) reports `expected number, found oversized number`. |
| `null` | Reports `expected string, found null`. Omit the member when you want an optional setting left unset. |
| Duplicate key | A list when the field is a list. A `duplicate field` error when the field is single-valued. |
| Type mismatch | A scalar where a nested object is expected reports `expected block, found string`. The expected side of a mismatch is shared across formats. |
| Comments | JSON has no comment syntax. Emitted JSON has no comments. [Templates](./templates.md#generating-a-template) covers what that means for a template. |

#### YAML

| Behavior | Detail |
|----------|--------|
| Document root | Must be a mapping. Any other root reports `expected a mapping at the document root`. |
| Empty document | An empty file, a whitespace-only file, and a file of comments each parse as a configuration that sets nothing, the way an empty TOML or HCL file does. |
| Multiple documents | A second document reports `expected a single document`. |
| Scalar resolution | A plain scalar resolves through the YAML 1.2 core schema. A quoted, literal, or folded scalar is a string whatever its text. `port: 8080` is an integer. `port: "8080"` is a string. |
| 1.1 literals | `yes`, `no`, `on`, and `off` are not in the 1.2 schema. `country: no` is the string `no`, not a boolean. `-.nan`, uppercase or signed base prefixes (`0X1F`, `-0x10`), and underscored numbers (`1_000`) are also strings. |
| Oversized integer | An integer beyond `i64` range reports `expected integer, found oversized integer`. |
| Oversized number | A decimal that overflows `f64` reports `expected number, found oversized number`. `.inf` written by an operator is the only infinity the model holds from YAML. |
| `null` | `null`, `~`, or a key with no value reports `expected string, found null`. |
| Duplicate key | A list when the field is a list. A `duplicate field` error when the field is single-valued. |
| Alias | Not expanded. Reports `expected string, found alias` at the alias. |
| Anchor | Read through. The anchored node is ordinary data. |
| Merge key `<<` | An ordinary key. A spec that does not declare it reports `unknown field: <<`. |
| Core schema tags | `!!str 8080` is the string `8080`. The non-specific `!` resolves the same way on a scalar. A core scalar tag whose text it cannot read (`!!int foo`), a core tag on the wrong node kind (`!!int {a: 1}`), and any tag outside the core schema each report `expected string, found tagged value`. |
| Non-scalar key | A mapping, sequence, or explicit `? *alias` key reports `expected a scalar key`. The entry is skipped so later errors are not hidden. An alias as a plain key (`*a: 1`) is a syntax error. A scalar key reads as its text whatever the schema would resolve it to. `8080:` names the field `8080`. |
| Type mismatch | A scalar where a nested mapping is expected reports `expected block, found string`. The expected side of a mismatch is shared across formats. |
| Emitted style | `emit_yaml` produces block-style YAML with two-space indentation, values before nested mappings, and a trailing newline. Every string emits double-quoted, so a value the schema would otherwise resolve (`no`, `123`, `null`) reads back as the string it was. |

:::note
`hcl-edit` rejects duplicate attribute keys while parsing, and TOML rejects a duplicate key the same way.
A repeated block parses, and confval reports it with a related span pointing at the first occurrence.
JSON and YAML both permit the same key twice.
A duplicate key parses and the spec's declared shape decides what it means.
:::

## Writing parsers by hand

Sometimes a block's remaining fields depend on the value of a discriminator field.
The `Spec` derive cannot express that shape, so you write the parser yourself.

By convention, confval reduces a spec's fields to primitive types wherever it can.
A spec holds the most broadly typed form of a field.
Lowering narrows that value to a more specific type once validation has run.

A discrete set of values follows the same pattern rather than becoming an enum in the spec.
Take a `mode` field that accepts "red", "green", or "blue" as an example.
The spec holds a `Located<String>`.
Validation checks it against a [KeywordSet](./validation.md#keywordset).
Lowering converts the string to an enum.

Handwritten parsers cover the shapes that pattern cannot express.
The clearest case is a block whose remaining fields depend on a discriminator.
The parser reads the discriminator first and dispatches on it.

A parser is an implementation of confval's `FromFields` trait:

```rust
pub trait FromFields: Sized {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self>;
}
```

An implementation parses every field before deciding what to return.
Parsing all of them first keeps one bad field from hiding the problems in the others.

Report each problem as you find it.
Then return `None` when a field failed and the value cannot be built.
The `None` itself holds no reason.
Whatever explains the failure must already be in the report.

A `Fields` built by `to_template` can also hold commented-out entries, fields whose `commented` flag is set.
Such a field reads as absent.
`Fields::get` and `Fields::has` skip one for you.
If you iterate with `Fields::iter`, check the flag and skip the field the way the generated walk does.

### The field model

A handwritten parser reads `Fields`, confval's format-neutral view of one level of structure.
A frontend builds it, and from there nothing knows which format the text was.

| Type | Role |
|------|------|
| `Fields` | One level: the named entries of a body, table, or inline object, plus the span a missing-field error points at. |
| `Field` | One entry: its name, the span of the name, the span of the whole entry, and a `FieldKind`. |
| `FieldKind` | Either `Value` for an attribute (`name = value`) or `Block` for a block (`name { ... }` in HCL, `[name]` in TOML). The split lets a diagnostic say "found block" rather than "found object". |
| `Value` | A span plus a `ValueKind`: a `Scalar`, a `Seq` (a list), a `Map` (nested fields), or `Other`. |
| `Scalar` | `String`, `Int(i64)`, `Float(f64)`, `Bool`, or `Unparsed`. Integers and floats stay distinct so a format that separates them (like TOML's `1` and `1.0`) round-trips faithfully. |
| `Unparsed(String)` | The raw text of a value from a source that only holds strings, such as an environment variable or a command line flag. The leaf parsers coerce it to the type they expect. No file frontend produces it. A quoted string in a file stays a `String`. |
| `Other(label)` | A value that exists in the file but falls outside the model, such as an HCL template or a TOML datetime. It surfaces as a type mismatch named by the label. For example, `expected string, found datetime`. [Format Limitations](./format-limitations.md) lists every one. |

### Helpers

confval ships helpers so a handwritten parser reports the same way the derive does.

**Leaf parsers** turn one `Field` into a `Located` value, reporting a typed error on mismatch:

| Helper | Parses |
|--------|--------|
| `parse_string_field` | a string |
| `parse_int_field` | an `i64` |
| `parse_float_field` | an `f64` |
| `parse_bool_field` | a boolean |
| `parse_path_field` | a `PathBuf` |
| `parse_string_list_field` | an array of strings |

**Structural parsers** recurse through `FromFields`:

| Helper | Parses |
|--------|--------|
| `parse_struct_field` | one nested struct from a block or a map value |
| `parse_single_struct` | same as `parse_struct_field`, with duplicate reporting |
| `parse_struct_list_field` | repeated blocks or a sequence of maps, into a `Vec` |

**Occurrence helpers** decide what a repeated field means:

| Helper | Behavior |
|--------|----------|
| `first_occurrence` | Records the first occurrence of a leaf field. Reports a later one as a duplicate. |
| `parse_single_struct` | The same guard around a nested block. |
| `parse_string_list_occurrence` | Accumulates a list field's occurrences into one list, in document order. |

The derive wraps every leaf arm it generates in `first_occurrence`.
A derived spec therefore reports a repeated field.
A handwritten parser that assigns its slot directly takes the last value instead, with no diagnostic.

**Reporting helpers** keep messages uniform: `report_unknown_field`, `report_missing_field`, `report_duplicate_field`.
Unknown fields are always errors.
There is no lenient mode.

The `handwritten` example calls these helpers against a tagged enum and a handwritten root.
The test at `crates/confval/tests/handwritten_parity.rs` writes one spec both ways and asserts that the two write walks render the same text and agree on every span.

## Writing emitters by hand

A type with a handwritten `FromFields` needs a handwritten `ToFields` too, because the derive generates one only for the types it parses.
`ToFields` has two required walks.
`to_fields` emits every field with its span detached.
This is the populated view and the template.
`to_source_fields` emits only the fields the source set, keeps their spans, and recurses into children with `to_source_fields` rather than `to_fields`.
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
| `string_list` | emits every element detached | emits the elements that keep a span, or omits the field |
| `string_list_opt` | emits detached when present | emits when the wrapper keeps a span, elements included |
| `block`, `block_opt`, `block_list` | recurses with `to_fields` | recurses with `to_source_fields` when the block keeps a span |
| `block_opt_default` | fills an absent block from `S::default()` | omits an absent block |
| `literal_string` | emits detached | emits detached |

`block_opt_default` is the counterpart of `#[confval(nested, default)]`.
Its populated walk shows the values the program will run with even for a block the operator never wrote.
Use `block_opt` for an optional block with no default.
Both walks omit an absent block.

`leaf` accepts the same types a derived spec field accepts, through the sealed `Leaf` trait: `String`, `i64`, `f64`, `bool`, and `PathBuf`.
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

The builder does not cover every shape a spec can hold.
A string-keyed map has a derive form, `#[confval(map)]`, so it needs no handwritten walk.
The builder has no method for one.
Build such a field directly with `Field::detached_value` or `Field::detached_block`.
Locate it with `at` when it keeps a span, then `push` it into the builder:

```rust
let field = Field::detached_value(name, value).at(span);
builder.push(field);
```

The walk does not reach a pushed field.
You decide what it holds.
On a source walk that includes deciding whether the source set it.
The builder still shapes every other field of the type.

`at` sets the field's span and its source.
An attribute's value takes the same span.
A block's nested level keeps its own source and enclosing span.
A sequence's elements keep the spans they were built with.

A handwritten spec type also implements `Validate` and `ValidateNested`.
`Validate` holds its rules.
`ValidateNested` holds the descent into its children, the traversal the derive would have written from the struct definition.
The `Self: ValidateNested` bound on `validate_all` makes omitting the traversal a compile error rather than a silently skipped subtree.
A type in a required nested slot also needs `Default`, because the generated parser fills an absent block with it before reporting the block missing.

`to_template` defaults to `to_fields` for a handwritten impl.
That fallback recurses with `to_fields`, so doc comments stop at the first handwritten node and never reach anything below it.
A derived block nested under a handwritten one renders without its comments.
The `handwritten` example prints both sides of that boundary.
