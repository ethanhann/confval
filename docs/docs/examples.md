---
sidebar_position: 4
---

# Examples

The crate ships six runnable examples in `crates/confval/examples/`.
`hcl`, `toml`, and `kdl` define the same types and differ only in the format they read.
The rest demonstrate one feature each.
Each section below gives the run command and the output it prints.

## hcl

The HCL example feeds an intentionally invalid document to show the rendered report.

```shell
cargo run -q -p confval --example hcl --features derive,color,hcl
```

Every problem is reported at its own line and column:

```shell
error: port must be at most 65535
 --> server.hcl:2:8
  |
2 | port = 99999
  |        ^^^^^
  = help: Set port to at most 65535

error: hostname must not be empty
 --> server.hcl:1:12
  |
1 | hostname = ""
  |            ^^
  = help: Set hostname to a reachable address, e.g. "127.0.0.1".

error: unknown mode: yolo
 --> server.hcl:5:10
  |
5 |   mode = "yolo"
  |          ^^^^^^
  = help: expected one of: enforce, log, off
```

## toml

The TOML example feeds a valid document to show the lowered config.

```shell
cargo run -q -p confval --example toml --features derive,color,toml
```

The run passes validation and prints the runtime values:

```shell
listening on 127.0.0.1:8080 with 8 workers
limits: max_body_mb=16 mode=enforce
```

## kdl

The KDL example renders the diagnostics for a failing variant to stderr, then feeds a valid document, prints the lowered config, and emits the populated spec back to canonical KDL.

```shell
cargo run -q -p confval --example kdl --features derive,color,kdl
```

The failing variant reports its problems the way the `hcl` example does, with spans into the KDL source.
The valid run then passes validation, prints the runtime values, and shows the write path:

```shell
listening on 127.0.0.1:8080 with 8 workers
limits: max_body_mb=16 mode=log

+ Emitted KDL:
hostname "127.0.0.1"
port 8080
workers 8
tls #false

limits {
  max_body_mb 16
  mode "log"
}
```

## issue_severity

The issue_severity example shows a warning.

```shell
cargo run -q -p confval --example issue_severity --features derive,color,toml
```

A warning renders in the report but does not stop the run:

```shell
warning: hostname set to listen on every available network device
 --> server.toml:1:12
  |
1 | hostname = "0.0.0.0"
  |            ^^^^^^^^^
  = help: This might be undesired.

listening on 0.0.0.0:8080 with 8 workers
limits: max_body_mb=16 mode=enforce
```

## validate_traversal

The validate_traversal example shows what `validate_all` reaches.

```shell
cargo run -q -p confval --example validate_traversal --features derive,color,toml
```

The same invalid nested block is validated twice, differing only in whether the enclosing block is enabled:

```shell
upstream enabled: the nested child is validated
error: attempts must be at most 10
 --> service.toml:5:12
  |
5 | attempts = 99
  |            ^^
  = help: Set attempts to at most 10

upstream disabled: descend breaks, so the child is skipped
no issues
```

## layering

The layering example assembles one config from a base file, a joined defaults file, the environment, and the command line.

```shell
cargo run -q -p confval --example layering --features derive,color,toml,layering
```

The environment sets `port` and the nested `limits.mode`, the command line sets `limits.max_body_mb` and `tls`, and the joined defaults file fills `workers`:

```shell
listening on 127.0.0.1:9090 with 8 workers
limits: max_body_mb=64 mode=log
tls: true
```

See [Layering](./guide/layering.md) for how the sources merge and how environment and command line values are coerced.

## templates

The templates example renders a spec back to configuration text.

```shell
cargo run -q -p confval --example templates --features derive,color,toml
```

The spec populates with its defaults, emits as plain TOML, and emits again as a template with each field's doc comment above it:

```shell
populated field model:
hostname = "127.0.0.1"
port = 8080
workers = 4
tls = false
limits {
  max_body_mb = 16
  mode = "enforce"
}

emitted TOML:
hostname = "127.0.0.1"
port = 8080
workers = 4
tls = false

[limits]
max_body_mb = 16
mode = "enforce"

emitted TOML template:
# The address the server binds to.
hostname = "127.0.0.1"
# The port the server listens on.
port = 8080
# The number of worker threads.
workers = 4
# Whether TLS is enabled.
tls = false

# Request size and mode limits.
[limits]
# The maximum request body size, in megabytes.
max_body_mb = 16
# How limit violations are handled.
mode = "enforce"
```

See [Templates](./guide/templates.md) for how `to_fields`, `to_template`, and the emitters fit together.

## Additional Examples

Additional examples are available for reference:

- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1).
- Snakeway reverse proxy's [snakeway-conf crate](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src) (advanced usage)
