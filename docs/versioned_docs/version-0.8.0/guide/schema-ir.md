---
sidebar_position: 8
---

# Schema IR

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

The schema carries a scalar leaf's default rendered to text.
The derive evaluates the default expression when `schema()` runs and stores the result on the field, so `#[confval(default = 4)]` reads back as `"4"`.
A defaulted list, map, or block carries no text, because there is no single value to render.
`has_default` still records that one applies.
A handwritten spec carries a default the same way, through `with_default_text` beside the other builder calls.
To render a whole document of defaults, use the [template](./templates.md) walk, `ServerSpec::default().to_template()`.

## When a field is required

`required` answers whether an absent field is a parse error.
A field is required when its shape needs a value and it declares no default.
An `Option` field, a zero-or-more block list, and any field with a `#[confval(default)]` are not required.

A defaulted field therefore reports `required` as false and `has_default` as true, whatever its shape.
An editor reads `required` to report only the fields the parser would reject as missing.

## Recording constraints

The derive cannot read a `Validate` body, so a closed-set field looks like a plain `Located<String>` and a numeric range is invisible to the schema.
Three attributes record a constraint on a scalar leaf so the schema can carry it.

`#[confval(keywords = PATH)]` names a `keyword_enum!` type and requires a `String` leaf.
The schema carries its allowed strings as `Constraint::Keywords`.

`#[confval(range = PATH)]` names a `RangeConstraint` and requires an `Int` or `Float` leaf.
The schema carries its bounds, units, and help line as `Constraint::Range`.

`#[confval(references = <block>)]` marks a `String` leaf whose value names another block by its label.
The `<block>` is the config field name of a labeled block, one that marks a child field with `#[confval(label)]`.
The schema carries the target as `Constraint::References`.

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

The attribute records the constraint for the schema.
On a derived spec the derive also runs the check during validation.
The `Validate` body therefore carries no line for that field.
A handwritten spec still calls the check itself, because the derive generates nothing for it.

## How a reference resolves

A reference names its target block by a bare name.
The name resolves outward from the reference's enclosing block.
The nearest enclosing scope whose schema declares a labeled block field of that name wins, and the root is searched last.
Labels are collected within that one scope instance.
So two sibling instances of the enclosing block may reuse a label, and a reference sees only the labels of its own scope.
A field of the same name that is not a labeled block does not stop the search, so a reference field may carry its target's name.

For example, a route names one of its own service's upstreams:

```rust
#[derive(confval::Spec)]
struct ServiceSpec {
    name: Located<String>,
    #[confval(nested)]
    upstreams: Vec<Located<UpstreamSpec>>,
    #[confval(nested)]
    routes: Vec<Located<RouteSpec>>,
}

#[derive(confval::Spec)]
struct UpstreamSpec {
    #[confval(label)]
    name: Located<String>,
    port: Located<i64>,
}

#[derive(confval::Spec)]
struct RouteSpec {
    #[confval(references = upstreams)]
    upstream: Located<String>,
}
```

Each route's `upstream` value resolves against the upstreams of its own service.
A label defined in a sibling service is out of reach, and the same label in two services is not a conflict.

## Running the reference check

`validate_all` does not run the reference check, because the check reads the whole document rather than one level's own fields.
After you parse and validate, call `check_references` with the parsed `Fields`, the schema, and the report:

```rust
use confval::pipeline::check_references;
use confval::schema::ToSchema;

if let Some(fields) = &fields {
    check_references(fields, &ServerSpec::schema(), &mut report);
}
```

The pass reports an undefined reference, a duplicate label, and an empty label, each at its value's span.
The language server runs the same pass in its diagnostics, so the editor and your pipeline report the same reference errors.

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

A reference field is declared the same way, with `Some(Constraint::References { block: "upstreams" })` as the constraint.
The target block marks its label child by calling `as_label()` on that child's `SchemaField`.
