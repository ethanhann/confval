---
sidebar_position: 4
---

# Examples

Fifteen runnable examples ship in the repository: fourteen in `crates/confval/examples/` and a runnable language server in `crates/confval-lsp/examples/`.

`hcl`, `toml`, `kdl`, `json`, and `yaml` are the same program five times.
Each renders the diagnostics for a failing variant to stderr, feeds a valid document, prints the lowered config, and emits the populated spec back to canonical text.
They differ in the source text, its file name, and the two format calls that parse and emit it.
The rest demonstrate one feature each, except `handwritten`, which runs the whole pipeline over a spec written without the derive.
Each section below gives the run command.
`just examples` runs them all.

## hcl

The `hcl` example runs those steps over HCL.

```shell
cargo run -q -p confval --example hcl --features derive,color,hcl
```

## toml

The `toml` example runs those steps over TOML.

```shell
cargo run -q -p confval --example toml --features derive,color,toml
```

## kdl

The `kdl` example runs those steps over KDL.

```shell
cargo run -q -p confval --example kdl --features derive,color,kdl
```

## json

The `json` example runs those steps over JSON.

```shell
cargo run -q -p confval --example json --features derive,color,json
```

## yaml

The `yaml` example runs those steps over YAML.

```shell
cargo run -q -p confval --example yaml --features derive,color,yaml
```

## issue_severity

The `issue_severity` example illustrates the difference between an error and a warning.

```shell
cargo run -q -p confval --example issue_severity --features derive,color,toml
```

## validate_traversal

The `validate_traversal` example shows what `validate_all` reaches.

```shell
cargo run -q -p confval --example validate_traversal --features derive,color,toml
```

## layering

The `layering` example assembles one config from a base file, a joined defaults file, the environment, and the command line.

```shell
cargo run -q -p confval --example layering --features derive,color,toml,layering
```

See [Layering](./guide/layering.md) for how the sources merge and how environment and command line values are coerced.

## templates

The `templates` example renders a spec back to configuration text.

```shell
cargo run -q -p confval --example templates --features derive,color,toml,hcl
```

The spec populates with its defaults and emits twice per format, once plain and once as a template with each field's doc comment above it.
The unset optional `pid_file` stays out of the plain form and renders in the template as a commented-out entry.

See [Templates](./guide/templates.md) for how `to_fields`, `to_template`, and the emitters fit together.

## doc_fallback

The `doc_fallback` example shows where a template block's comment comes from.

```shell
cargo run -q -p confval --example doc_fallback --features derive,toml
```

## json_diagnostics

The `json_diagnostics` example renders a report as JSON for CI and tooling.

```shell
cargo run -q -p confval --example json_diagnostics --features derive,serde,toml
```

## narrow

The `narrow` example shows the ready-made narrowing helpers that convert spec integers to the widths a runtime type needs.
It exercises five of them.
The remaining integer widths and their `opt_` variants share the same shape.

```shell
cargo run -q -p confval --example narrow --features derive,color,toml
```

## representations

The `representations` example prints the three views of one loaded spec: the source view of what was set, the populated view after defaults, and the runtime view of the lowered values.

```shell
cargo run -q -p confval --example representations --features derive,serde,toml
```

## handwritten

The handwritten example writes a spec without the derive, for a block whose `mode` field decides which fields the rest of the block has.
Each level of its tree is written the other way from the level above: the root is handwritten, its children are derived, and the `tls` block inside a derived route is handwritten again.
It prints the diagnostics, the runtime config, the populated and source views, the comments a handwritten node drops from a template, and the same model in HCL.

```shell
cargo run -q -p confval --example handwritten --features derive,color,toml,hcl
```

## serve

The serve example runs the language server over stdio against a demo spec, so you can point an editor at a running server before writing your own.
Pick a format, then launch an LSP client at the built binary.
[The language server core](./guide/language-server.md#trying-it-against-an-editor) walks through an editor setup.

```shell
cargo run -p confval-lsp --example serve hcl
```

## Additional Examples

Additional examples are available for reference:

- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1).
- Snakeway reverse proxy's [snakeway-conf crate](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src) (advanced usage)
