---
sidebar_position: 9
---

# Format Limitations

Sometimes you parse a configuration in one format and emit it in another, or you generate a template and wonder whether the write can fail.
The formats do not share one vocabulary.
TOML has a datetime literal and JSON does not.
KDL, TOML, and YAML write infinity where JSON and HCL have no token for it.
This page lists the gaps format by format, so you can see which conversions fail before you run one.

A value that cannot be expressed produces an error naming the value and its dotted path.
This holds in every format.
Nothing is rounded, approximated, or silently dropped.

## Values outside the model

Every frontend parses into the same neutral field model, and the model's scalars are strings, `i64` integers, `f64` floats, and booleans.
A source value outside that set still parses, but it is held as an opaque marker with a label rather than a value.

| Format | Source value                                                | Label               |
|--------|-------------------------------------------------------------|---------------------|
| TOML   | a datetime                                                  | `datetime`          |
| TOML   | a value with no neutral scalar                              | `value`             |
| HCL    | `null`                                                      | `null`              |
| HCL    | a string template or heredoc                                | `string template`   |
| HCL    | a number with no `i64` or `f64` value                       | `number`            |
| HCL    | any other expression, such as a function call or a variable | `expression`        |
| KDL    | `#null`                                                     | `null`              |
| KDL    | an integer beyond `i64`                                     | `oversized integer` |
| JSON   | `null`                                                      | `null`              |
| JSON   | an integer beyond `i64`                                     | `oversized integer` |
| JSON   | a number whose `f64` value is not finite                    | `oversized number`  |
| YAML   | `null`, `~`, or a key with no value                         | `null`              |
| YAML   | an integer beyond `i64`                                     | `oversized integer` |
| YAML   | a decimal float that overflows `f64`                        | `oversized number`  |
| YAML   | an alias, `*name`                                           | `alias`             |
| YAML   | a tag the frontend refuses                                  | `tagged value`      |

A marker is not an error by itself.
It surfaces as an ordinary type mismatch when a spec field reads it, so `"when": null` under a string field reports `expected string, found null` at the value.
It also refuses to emit to every format, because there is nothing faithful to write.

## Values a format cannot write

The model can hold a value that a target format has no literal for.
Emitting one returns an `EmitError` rather than inventing a syntax for it.

| Target | Cannot write                                                                                         | Writes without trouble                                                       |
|--------|------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------|
| TOML   | nothing beyond the markers above                                                                     | non-finite floats, `i64::MIN`, nested arrays, any key                        |
| HCL    | a non-finite float, `i64::MIN`                                                                       | nested arrays, mixed arrays                                                  |
| KDL    | an array inside an array, an array mixing scalars and objects, an object inside a grouped repetition | non-finite floats, as `#inf`, `#-inf`, and `#nan`                            |
| JSON   | a non-finite float                                                                                   | everything else, `i64::MIN` included                                         |
| YAML   | nothing beyond the markers above                                                                     | non-finite floats, as `.inf`, `-.inf`, and `.nan`, nested sequences, any key |

KDL's gaps all follow from one rule.
A KDL argument must be a scalar, and the language has no inline array literal, so there is no way to write an inner array or an object inside a grouped repetition.
YAML has no gap in this table, and it carries two markers no other format produces.
An alias is not expanded, and a tag outside the core schema has no reading.
A decimal that overflows `f64` refuses rather than becoming an infinity the operator never wrote, which JSON does too.

HCL rejects `i64::MIN` because its parser reads the literal as a negation applied to a number that overflows on the way back in.

For example, a KDL config with `rate #inf` converts to TOML, where it emits as `inf`.
Converting the same config to JSON returns an error at `rate`, because JSON's grammar has no token for infinity.

## Names and repetition

A name can also be one the target cannot write.

HCL attribute and block names must be identifiers.
TOML, KDL, JSON, and YAML quote any name, so a field named `not an ident` emits to all four and fails to HCL alone.
This is the only source of an `UnrepresentableName` error.

Repetition is format-specific, because each format refuses the shapes it would otherwise collapse silently.

- TOML refuses a value beside a same-named block, two same-named values, and any repetition inside an inline table.
- HCL repeats blocks freely and writes a value next to a same-named block, but it refuses a duplicate attribute name and any repetition inside an object.
- JSON refuses a value beside a same-named block, because the only way to write it is a duplicate key, which most consumers collapse to one member. Repeated values and repeated blocks group into arrays instead.
- YAML refuses a value beside a same-named block, for the same reason JSON does. Repeated values and repeated blocks group into sequences instead.
- KDL writes every repetition but one. Repeated values group into one node's arguments, repeated blocks are the native list form, and a value beside a same-named block emits as two nodes. A grouped repetition holding an object cannot be written, because an argument must be a scalar.

These shapes cannot come from a populated spec.
They arise only when you emit a tree parsed from a format that permits them, or one you built by hand.

## What template generation can rely on

A populated spec is the tree `to_fields` or `to_template` builds from your spec types and their defaults.
It stays inside the vocabulary the formats share.
Names are Rust identifiers, values are ordinary scalars, and nothing repeats.

| Target | A populated spec fails when                                             |
|--------|-------------------------------------------------------------------------|
| TOML   | never                                                                   |
| KDL    | never                                                                   |
| YAML   | never                                                                   |
| JSON   | a float default is infinity or NaN                                      |
| HCL    | a float default is infinity or NaN, or an integer default is `i64::MIN` |

If your defaults are ordinary numbers, template generation cannot fail, so you can `expect` on the emit call.

## What is dropped by design

A few things are lost in conversion without an error, because they are presentation rather than configuration.

- Operator layout and comments. Emit writes canonical text, and a parsed file's formatting is never held in the model.
- Doc comments in JSON. The other formats render template annotations as comments, and JSON has no comment syntax, so a JSON template equals the populated output.
- Which of two nesting syntaxes the source used. A TOML `[table]` and an inline table, an HCL block and an object attribute, or a YAML block mapping and a flow mapping all read as the same structure and emit in the target's canonical form.
- Separate duplicate keys. JSON, YAML, and KDL group repeated names into one list on emit, so a list-shaped field reads the same list it would have. A single-value field trades its `duplicate field` report for a type mismatch on reparse, because the grouped member is an array where a scalar is expected.
- The type of a layered override. Text from an environment variable or a command line flag reaches the model unparsed, and every format writes it as a string, so a typed reparse of the emitted file reads those leaves as strings.
