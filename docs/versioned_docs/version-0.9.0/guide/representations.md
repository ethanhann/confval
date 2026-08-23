---
sidebar_position: 7
---

# Representations

When you need to inspect a running service's configuration, the file on disk may not match what the service loaded.
Three value representations of one loaded spec are available:

1. The source view shows the configuration exactly as the operator wrote it, with no defaults applied.
2. The populated view shows the configuration the service resolved to, with every default filled.
3. The runtime view shows the typed values the program uses.

A fourth view, the schema view, reads the type rather than a value.
See [The schema view](#the-schema-view) below.

The `representations` example prints all three value views from one spec.

Run it with:

```shell
cargo run -q -p confval --example representations --features derive,serde,toml
```

The source sets only `mode`, leaving `max_body_mb` to its default.
The views differ exactly where a default fills a gap:

```text
+ Source view (what was set):
mode = "log"

+ Populated view (after defaults):
max_body_mb = 16
mode = "log"

+ Runtime view (what runs):
{
  "max_body_mb": 16,
  "mode": "log"
}
```

## The source view

`to_source_fields` returns a `Fields` holding only the fields the source set.
`#[derive(Spec)]` generates it.
The prelude exports the `ToFields` trait that declares it, so `spec.to_source_fields()` works wherever the prelude is in scope.
The result is the same format-neutral field model a frontend produces, and the ordinary emit functions render it in any format.

```rust
use confval::format::toml::{emit_toml, parse_toml};

let spec: LimitsSpec = parse_toml(&sources, id, &mut report).unwrap();
let source = emit_toml(&spec.to_source_fields())?;
```

The view is decided one field at a time by whether the field's span is attached.
Parsing gives every value it reads a real span into the source file.
Every filled default carries a detached sentinel span instead.
The source view keeps the attached values and drops the detached ones, so a default never appears as though the operator wrote it.

Each value the view keeps carries its real source span.
A tool that wants to report where a value came from still has the location.
A block the operator wrote with every inner field left to its default renders as an empty block, because the block itself was written but nothing inside it was.

### Bare lists and the source view

A bare `Vec<Located<String>>` list holds no span of its own.
An empty list the operator wrote is indistinguishable from an absent one, and both are dropped.
When the difference matters, use the wrapped form, `Option<Located<Vec<Located<String>>>>`.
The wrapped form keeps the list's own span and survives the source view even when empty.

## The populated view

The populated view is the plain [populate](./templates.md) dump.
`to_fields` fills every default the source omitted, and the emitters render it.

```rust
let populated = emit_toml(&spec.to_fields())?;
```

Where the source view answers what was set, the populated view answers what the service resolved to.
A field the operator left out appears here with its default value.

## The runtime view

The runtime view is the lowered config serialized with serde.
A lowered config holds plain runtime types.
Deriving `serde::Serialize` on the config struct is the whole mechanism.

```rust
#[derive(confval::Config, serde::Serialize)]
#[confval(lower_from = LimitsSpec)]
struct LimitsConfig {
    #[confval(lower(from = max_body_mb, with = narrow::i64_to_u16))]
    max_body_mb: u16,
    #[confval(lower(from = mode, with = narrow::keyword::<Mode>))]
    mode: Mode,
}
```

A `keyword_enum!` type serializes as its keyword string rather than its Rust variant name.
The runtime view shows a mode as `"log"`, exactly as the config file and the other two views do.
This impl is behind confval's `serde` feature, so it appears only when you enable serde.

## The schema view

The three views above read a value.
The schema view reads the type.
`ServerSpec::schema()` is an associated function with no instance.
It returns a `Schema` that names each field, whether it is required, and the kind it holds.

For example, ask a spec type for its schema:

```rust
use confval::schema::ToSchema;

let schema = ServerSpec::schema();
```

The [schema IR](./schema-ir.md) page covers what it carries, the attributes that declare a field's constraint and run it during validation, and why it needs no instance.

## Why a separate walk

The populated view and the source view read the same spec, but neither can produce the other.
The populate walk fills defaults and detaches every span, removing any record of what the source set.
The source walk reads the spec's spans directly, which is where the set-or-defaulted distinction lives.

`to_source_fields` is therefore its own walk rather than a filter over the populated model.
It is a required method on `ToFields`, because no default body could answer the question without reporting defaults as operator-written.

A spec with a handwritten `ToFields` writes the source walk itself.
Build it with `FieldsBuilder`, which takes the walk as a parameter and applies this rule per field, as [Writing emitters by hand](./parsing.md#writing-emitters-by-hand) describes.
