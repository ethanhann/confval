# The frontends, and what each format can express

You are choosing the frontend feature that matches your project's configuration format, and you need to know where a format constrains what a spec can hold.
This file carries one row per frontend and the per-format limitations that change what a spec can express.

It does not restate how each format maps onto the field model.
That detail is in the complete confval documentation at https://ethanhann.com/confval/llms-full.txt.
Read it when you need the exact spelling a format uses for a nested block or a list.
That file tracks the latest release, so confirm any API against the confval version this project pins.

## One row per frontend

Each frontend is an independent feature.
Enable the one for the format you parse, and nothing pulls in a parser you do not use.

| Format | Feature | Parse function | Emit function | Emit of a populated spec can fail |
|--------|---------|----------------|---------------|-----------------------------------|
| HCL    | `hcl`   | `confval::format::hcl::parse_hcl`   | `emit_hcl`  | yes, for `i64::MIN` or a non-finite float default |
| TOML   | `toml`  | `confval::format::toml::parse_toml` | `emit_toml` | no |
| KDL    | `kdl`   | `confval::format::kdl::parse_kdl`   | `emit_kdl`  | no |
| JSON   | `json`  | `confval::format::json::parse_json` | `emit_json` | yes, for a non-finite float default |
| YAML   | `yaml`  | `confval::format::yaml::parse_yaml` | `emit_yaml` | no |

Each parse function takes a `&SourceMap`, a `SourceId`, and a `&mut Report`, and returns your spec as an `Option`.
The result is the same whichever frontend ran, so validation and lowering never depend on the format.

The emit column matters only when you generate a template.
Emitting a populated spec fails only for a numeric default the target format has no literal for.
If your defaults are ordinary numbers, TOML, KDL, and YAML never fail, HCL fails only on `i64::MIN` or a non-finite float, and JSON fails only on a non-finite float.
When your defaults are ordinary you may `expect` on the emit call.

## Duplicate keys

A format decides what a repeated name means, and confval reports it against the field's declared shape.

HCL and TOML reject a duplicate attribute key while parsing, so a repeated leaf is a syntax error.
A repeated block parses, and confval reports it with a related span pointing at the first occurrence when the field is not a list.
JSON and YAML permit the same key twice, so a duplicate key parses and the spec's shape decides.
A duplicate key is a list when the field is a list and a `duplicate field` error when it is not.
A repeated KDL node follows the same rule.

The practical consequence for a spec is that a list field accepts the repetition and a single-value field reports it.
Declare a field `Vec<Located<String>>` when the format may legitimately repeat it.

## Nesting spellings

Operators write a nested block in more than one way, and the field model normalizes them, so one nested spec accepts every spelling with identical spans.

- HCL writes a block, `limits { ... }`, or an attribute set to an object, `limits = { ... }`.
- TOML writes a `[table]`, an inline `{ ... }`, or an array of tables, `[[repeated]]`, which fills a `Vec` of nested structs.
- KDL writes a children block, `limits { ... }`, or properties on one node, `limits key=value`.
- JSON writes an object, the one way it nests.
- YAML writes a block mapping or a flow mapping.

You write `#[confval(nested)]` once, and every format's spelling reads into it.

## Values outside the model

The model's scalars are `String`, `i64`, `f64`, and `bool`.
A source value outside that set still parses, but it is held as an opaque marker rather than a value, and it surfaces as a type mismatch when a spec field reads it.
A `null` under a string field reports `expected string, found null`.

| Format | Values with no neutral scalar |
|--------|-------------------------------|
| TOML | a datetime, a value with no neutral scalar |
| HCL  | `null`, a string template or heredoc, a number with no `i64` or `f64` value, any other expression |
| KDL  | `#null`, an integer beyond `i64` |
| JSON | `null`, an integer beyond `i64`, a number whose `f64` value is not finite |
| YAML | `null` or `~`, an integer beyond `i64`, an overflowing decimal, an alias, a refused tag |

The consequence for a spec is that these values have no field shape to hold them.
Omit an optional member rather than writing `null`, because the model has no null and a `null` reads as a type mismatch on whatever field consumed it.

## Parsing and emitting one file

This program parses a TOML document into a spec and emits the populated spec back as text.
Swap `parse_toml` and `emit_toml` for another row's pair to read and write a different format.

```rust
use confval::prelude::*;

#[derive(confval::Spec)]
#[confval(derive_default)]
struct ServerSpec {
    #[confval(default = "127.0.0.1".to_string())]
    hostname: Located<String>,
    #[confval(default = 8080)]
    port: Located<i64>,
}

fn main() {
    let text = "hostname = \"0.0.0.0\"\nport = 9000\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", text);

    let spec: ServerSpec = confval::format::toml::parse_toml(&sources, id, &mut report)
        .expect("valid document parses");

    // A populated spec of ordinary scalars always emits to TOML.
    let out = confval::format::toml::emit_toml(&spec.to_fields()).expect("populated spec emits");
    println!("{out}");
}
```
