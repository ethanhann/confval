---
sidebar_position: 8
---

# The Schema IR

Sometimes you need the type of a spec rather than a value of it.
An editor writing completions is the case that drives this.
Before an operator writes a value, the editor needs to know which fields are legal here, which are required, what kind each one holds, and which values a closed-set field accepts.

The two value walks cannot answer that.
`FromFields` reads a `Fields` and builds a spec, and `ToFields` walks a spec instance and builds a `Fields`.
Both need an instance, because both read a value, and a populated `Fields` carries values, not declared types.

The schema IR is a third walk that reads the type.
`ToSchema::schema()` returns a `Schema`, a type-level description derived from the struct rather than from an instance.
It is an associated function with no `self`, because it describes a type and needs no value to read.

```rust
use confval::schema::ToSchema;

let schema = ServerSpec::schema();
for field in &schema.fields {
    println!("{}: required={}", field.name, field.required);
}
```

## What the IR carries

A `Schema` is one structural level: the spec type's own doc comment and its fields in declaration order.
Each `SchemaField` carries the field name as it appears in a config file, the field's own doc comment, whether it is required, whether it declares a default, and its declared type.

The declared type is a `SchemaType`.
A scalar leaf carries its `ScalarType` and the constraint it declares, if any.
A string list is `StringList`, an open-ended string-keyed map is `StringMap`, and a nested block is `Block`, which holds the child level's own `Schema` and a `repeated` flag that is true for a zero-or-more block list.

Each leaf reads its `ScalarType` from the Rust type, so `port: Located<i64>` is `Int` and `hostname: Located<String>` is `String`, with no dependence on any value.
A `PathBuf` leaf reads as `Path`, because the IR names the config-level type an operator writes, a path string, rather than the Rust wrapper.

The `Block` variant recurses into the child's own `schema()` through the `ToSchema` bound, so one call at the root builds the whole tree.

## Required folds in the default

`required` answers whether an absent field is a parse error.
It is `structurally_required && !has_default`.
A field with a `#[confval(default)]` is filled when absent rather than reported missing, whatever its shape, so it is not required.

So an `Option` field is not required, a zero-or-more block list is not required, and a defaulted field is not required even when its shape sits in the required column.
A defaulted list, a defaulted map, and a defaulted scalar all report `required` as false, and `has_default` as true.

Reading `required` straight from the shape would mark every defaulted field as a missing-field error to the editor, which is why the flag folds the default in.

## Why the IR carries no default value

The IR records whether a field has a default, not what the default renders to.
The value walks already produce the rendered text.
`RootSpec::default().to_template()` fills each default into a `Fields`, so a consumer that wants the concrete default reads the [template](./templates.md) walk.

This division keeps the derive from evaluating an arbitrary `#[confval(default = expr)]` at the type level, where no instance exists to evaluate it against.

## The two recording attributes

A constraint in confval is imperative.
It lives inside a `Validate` impl body, where the derive cannot read it, so a closed-set field is a plain `Located<String>` to a type-level walk and a numeric range is invisible.
Two recording attributes link a scalar leaf to its declared constraint so the IR can carry it.

`#[confval(keywords = PATH)]` names a `keyword_enum!` type and requires a `String` leaf.
The walk reads the `KEYWORDS` const the macro generates and carries it as `Constraint::Keywords`.

`#[confval(range = PATH)]` names a `RangeConstraint` value and requires an `Int` or `Float` leaf.
The walk renders the bounds to text and carries them as `Constraint::Range`, with the constraint's units and help line.

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

An attribute on the wrong leaf, or on a list, a map, or a nested block, is a compile error that names the mismatch.
A `keywords` and a `range` on one field is such an error, because one leaf cannot be both a string and a number.

## One checker, and the drift it leaves

The recording attribute records the association only.
The `Validate` body still performs the check, so the IR reads which constraint a field declares without moving the check that enforces it.
This keeps the `Validate` body as the single checker and changes no validation behavior.

The association then appears in two places, the attribute and the `Validate` body, and no guard links them.
A fixture test can assert the two agree in one spec, but it proves nothing about a downstream spec.
A spec author who omits the attribute gets no completion for that field, and one who names a keyword set the `Validate` body does not check gets a wrong completion, with no compile error either way.
This unguarded drift is the price of keeping the `Validate` body as the single checker.

## The nodes are a published surface

Every node type is `#[non_exhaustive]`.
A later release can add a variant to `SchemaType`, `ScalarType`, or `Constraint`, and a field to `Schema` or `SchemaField`, without a break.
Because the two node structs are `#[non_exhaustive]`, no crate outside confval builds a `Schema` or `SchemaField` with a struct literal.

A producer builds through the `Schema::new` and `SchemaField::new` constructors, the way the generated walk does.
A consumer reads the fields, so a test asserts by reading a field rather than by constructing an expected node.

## Handwritten specs

`#[derive(Spec)]` emits an `impl ToSchema` for every spec, the way it emits `impl ToFields`.
A parent's generated `schema()` calls the child's, so a handwritten spec nested inside a derived parent implements `ToSchema` by hand, exactly as it already implements `ToFields`.
A handwritten spec then implements five traits, `FromFields`, `ToFields`, `ToSchema`, `Validate`, and `ValidateNested`, and it builds its `Schema` through the constructors.

```rust
use confval::schema::{Schema, SchemaField, SchemaType, ScalarType, ToSchema};

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
                    constraint: None,
                },
            )],
        )
    }
}
```
