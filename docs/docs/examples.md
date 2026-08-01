---
sidebar_position: 4
---

# Examples

The crate ships eleven runnable examples in `crates/confval/examples/`.
`hcl`, `toml`, and `kdl` define the same types and differ only in the format they read.
The rest demonstrate one feature each.
Each section below gives the run command.
`just examples` runs them all.

## hcl

The HCL example renders the diagnostics for a failing variant to stderr, then feeds a valid document and prints the lowered config.

```shell
cargo run -q -p confval --example hcl --features derive,color,hcl
```

## toml

The TOML example feeds a valid document to show the lowered config.

```shell
cargo run -q -p confval --example toml --features derive,color,toml
```

## kdl

The KDL example renders the diagnostics for a failing variant to stderr, then feeds a valid document, prints the lowered config, and emits the populated spec back to canonical KDL.

```shell
cargo run -q -p confval --example kdl --features derive,color,kdl
```

## issue_severity

The issue_severity example shows a warning.

```shell
cargo run -q -p confval --example issue_severity --features derive,color,toml
```

## validate_traversal

The validate_traversal example shows what `validate_all` reaches.

```shell
cargo run -q -p confval --example validate_traversal --features derive,color,toml
```

## layering

The layering example assembles one config from a base file, a joined defaults file, the environment, and the command line.

```shell
cargo run -q -p confval --example layering --features derive,color,toml,layering
```

See [Layering](./guide/layering.md) for how the sources merge and how environment and command line values are coerced.

## templates

The templates example renders a spec back to configuration text.

```shell
cargo run -q -p confval --example templates --features derive,color,toml,hcl
```

The spec populates with its defaults and emits twice per format, once plain and once as a template with each field's doc comment above it.
The unset optional `pid_file` stays out of the plain form and renders in the template as a commented-out entry.

See [Templates](./guide/templates.md) for how `to_fields`, `to_template`, and the emitters fit together.

## doc_fallback

The doc_fallback example shows where a template block's comment comes from.

```shell
cargo run -q -p confval --example doc_fallback --features derive,toml
```

## json_diagnostics

The json_diagnostics example renders a report as JSON for CI and tooling.

```shell
cargo run -q -p confval --example json_diagnostics --features derive,serde,toml
```

## narrow

The narrow example shows the ready-made narrowing helpers that convert spec integers to the widths a runtime type needs.
It exercises five of them, and the remaining integer widths and their `opt_` variants share the same shape.

```shell
cargo run -q -p confval --example narrow --features derive,color,toml
```

## representations

The representations example prints the three views of one loaded spec: the source view of what was set, the populated view after defaults, and the runtime view of the lowered values.

```shell
cargo run -q -p confval --example representations --features derive,serde,toml
```

## Additional Examples

Additional examples are available for reference:

- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1).
- Snakeway reverse proxy's [snakeway-conf crate](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src) (advanced usage)
