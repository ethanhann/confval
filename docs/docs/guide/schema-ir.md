---
sidebar_position: 8
---

# The Schema IR

Sometimes you need the type of a spec rather than a value of it.
An editor writing completions is one example.
Before an operator writes a value, the editor needs to know which fields are legal, which are required, what kind each one holds, and which values a closed-set field accepts.

The value walks cannot answer that.
`FromFields` reads a `Fields` and builds a spec, and `ToFields` walks a spec and builds a `Fields`.
Both need an instance, and a populated `Fields` holds values, not declared types.

The schema IR reads the type instead.
`ToSchema::schema()` returns a `Schema` that describes the spec.
It is an associated function with no `self`, so you call it without a value.

For example, list the top-level fields and whether each is required:

```rust
use confval::schema::ToSchema;

let schema = ServerSpec::schema();
for field in &schema.fields {
    println!("{}: required={}", field.name, field.required);
}
```

## What a schema carries

A `Schema` is one level of a spec: the type's doc comment and its fields in declaration order.
Each `SchemaField` carries the field name as it appears in a config file, the field's doc comment, whether it is required, whether it declares a default, and its declared type.

The declared type is a `SchemaType`.
A scalar leaf carries its `ScalarType` and any constraint it declares.
A string list is `StringList`, and a string-keyed map is `StringMap`.
A nested block is `Block`, which holds the child level's own `Schema` and a `repeated` flag for a zero-or-more block list.

A leaf reads its `ScalarType` from the Rust type, so `port: Located<i64>` is `Int` and `hostname: Located<String>` is `String`.
A `PathBuf` leaf reads as `Path`, the name for the path string an operator writes.

A block recurses into the child's own `schema()`, so one call at the root builds the whole tree.

The schema does not carry rendered default values.
It records only whether a field has a default.
To read the concrete default text, use the [template](./templates.md) walk, `ServerSpec::default().to_template()`.

## When a field is required

`required` answers whether an absent field is a parse error.
A field is required when its shape needs a value and it declares no default.
An `Option` field, a zero-or-more block list, and any field with a `#[confval(default)]` are not required.

A defaulted field therefore reports `required` as false and `has_default` as true, whatever its shape.
An editor reads `required` to report only the fields the parser would reject as missing.

## Recording constraints

The derive cannot read a `Validate` body, so a closed-set field looks like a plain `Located<String>` and a numeric range is invisible to the schema.
Two attributes record a constraint on a scalar leaf so the schema can carry it.

`#[confval(keywords = PATH)]` names a `keyword_enum!` type and requires a `String` leaf.
The schema carries its allowed strings as `Constraint::Keywords`.

`#[confval(range = PATH)]` names a `RangeConstraint` and requires an `Int` or `Float` leaf.
The schema carries its bounds, units, and help line as `Constraint::Range`.

For example, attach a range to two integer fields:

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    #[confval(range = PORT)]
    port: Located<i64>,
    #[confval(default = 4, range = WORKERS)]
    workers: Located<i64>,
}
```

An attribute on the wrong leaf, or on a list, a map, or a block, is a compile error.

The attribute records the constraint for the schema, and on a derived spec the derive also runs the check during validation, so the attribute is the single source and the `Validate` body carries no line for it.
A handwritten spec still calls the check itself, because the derive generates nothing for it.

## Building and reading a schema

The node types are `#[non_exhaustive]`.
Build a `Schema` or a `SchemaField` through `Schema::new` and `SchemaField::new` rather than a struct literal.
Read a node by its fields, and match a `SchemaType`, `ScalarType`, or `Constraint` with a wildcard arm.
Your code then keeps compiling when a release adds a variant or a field.

## Handwritten specs

`#[derive(Spec)]` writes `ToSchema` for you.
A spec you write by hand implements it too, because a derived parent's `schema()` calls its child's.
Build the tree through the same constructors.

```rust
use confval::schema::{Constraint, Schema, SchemaField, SchemaType, ScalarType, ToSchema};

impl ToSchema for TlsSpec {
    fn schema() -> Schema {
        Schema::new(
            None,
            vec![SchemaField::new(
                "mode".to_string(),
                None,
                true,
                false,
                SchemaType::Scalar {
                    leaf: ScalarType::String,
                    constraint: Some(Constraint::Keywords(&["manual", "acme"])),
                },
            )],
        )
    }
}
```
