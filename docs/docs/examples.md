---
sidebar_position: 4
---

# Examples

The crate ships ten runnable examples in `crates/confval/examples/`.
`hcl`, `toml`, and `kdl` define the same types and differ only in the format they read.
The rest demonstrate one feature each.
Each section below gives the run command and the output it prints.

## hcl

The HCL example renders the diagnostics for a failing variant to stderr, then feeds a valid document and prints the lowered config.

```shell
cargo run -q -p confval --example hcl --features derive,color,hcl
```

Every problem is reported at its own line and column.
The empty `allow` entry is reported at that element, and the cross-field warning carries a related span pointing at the setting that caused it:

```shell
+ Diagnostics for a failing variant:
error: hostname must not be empty
 --> broken.hcl:1:12
  |
1 | hostname = ""
  |            ^^
  = help: Set hostname to a reachable address, e.g. "127.0.0.1".

error: allow entries must not be empty
 --> broken.hcl:4:24
  |
4 | allow = ["10.0.0.0/8", ""]
  |                        ^^
  = help: Remove the entry or set it to a network, e.g. "10.0.0.0/8".

warning: tls is enabled on port 80, which conventionally serves plaintext HTTP
 --> broken.hcl:2:8
  |
2 | port = 80
  |        ^^
  = help: Serve TLS on port 443 or another port.
 --> broken.hcl:3:7 (tls is enabled here)
  |
3 | tls = true
  |       ----

error: unknown mode: yolo
 --> broken.hcl:7:10
  |
7 |   mode = "yolo"
  |          ^^^^^^
  = help: expected one of: enforce, log, off
```

The valid run then passes validation and prints the runtime values:

```shell
listening on 127.0.0.1:8443 with 4 workers
allow: 10.0.0.0/8, 192.168.0.0/16
limits: max_body_mb=16 mode=enforce
```

## toml

The TOML example feeds a valid document to show the lowered config.

```shell
cargo run -q -p confval --example toml --features derive,color,toml
```

The run passes validation and prints the runtime values, including the lowered `allow` list:

```shell
listening on 127.0.0.1:8080 with 8 workers
allow: 10.0.0.0/8, 192.168.0.0/16
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
allow: 10.0.0.0/8, 192.168.0.0/16
limits: max_body_mb=16 mode=log

+ Emitted KDL:
hostname "127.0.0.1"
port 8080
workers 8
tls #false
allow "10.0.0.0/8" "192.168.0.0/16"

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
cargo run -q -p confval --example templates --features derive,color,toml,hcl
```

The spec populates with its defaults and emits twice per format, once plain and once as a template with each field's doc comment above it.
The unset optional `pid_file` stays out of the plain form and renders in the template as a commented-out entry:

```shell
+ Emitted TOML:
hostname = "127.0.0.1"
port = 8080
workers = 4
tls = false

[limits]
max_body_mb = 16
mode = "enforce"

+ Emitted TOML template with annotations:
# The address the server binds to.
hostname = "127.0.0.1"
# The port the server listens on.
port = 8080
# The number of worker threads.
workers = 4
# Whether TLS is enabled.
tls = false
# The PID file path. Left unset here, so the template renders it as a
# commented-out entry rather than hiding it.
#pid_file = ""

# Request size and mode limits.
[limits]
# The maximum request body size, in megabytes.
max_body_mb = 16
# How limit violations are handled.
mode = "enforce"

+ Emitted HCL:
hostname = "127.0.0.1"
port = 8080
workers = 4
tls = false

limits {
  max_body_mb = 16
  mode = "enforce"
}

+ Emitted HCL template with annotations:
# The address the server binds to.
hostname = "127.0.0.1"
# The port the server listens on.
port = 8080
# The number of worker threads.
workers = 4
# Whether TLS is enabled.
tls = false

# The PID file path. Left unset here, so the template renders it as a
# commented-out entry rather than hiding it.
#pid_file = ""
# Request size and mode limits.
limits {
  # The maximum request body size, in megabytes.
  max_body_mb = 16
  # How limit violations are handled.
  mode = "enforce"
}
```

See [Templates](./guide/templates.md) for how `to_fields`, `to_template`, and the emitters fit together.

## doc_fallback

The doc_fallback example shows where a template block's comment comes from.

```shell
cargo run -q -p confval --example doc_fallback --features derive,toml
```

Both sprocket fields embed the same spec.
The primary field carries its own doc comment, and the secondary field falls back to the struct doc on the embedded spec:

```shell
max_weight = 16

# The primary sprocket. A field doc wins over the struct doc on
# `SprocketSpec`.
[primary_sprocket]
max_height = 32

# A sprocket's dimensions. The `secondary_sprocket` field has no doc, so its
# block falls back to this comment.
[secondary_sprocket]
max_height = 32
```

## json_diagnostics

The json_diagnostics example renders a report as JSON for CI and tooling.

```shell
cargo run -q -p confval --example json_diagnostics --features derive,serde,toml
```

Each issue carries its resolved source name, line, and column alongside the raw byte offsets:

```json
{
  "issues": [
    {
      "severity": "error",
      "message": "port must be at most 65535",
      "location": {
        "source": "server.toml",
        "line": 2,
        "column": 8,
        "start": 21,
        "end": 26
      },
      "help": "Set port to at most 65535"
    },
    {
      "severity": "error",
      "message": "hostname must not be empty",
      "location": {
        "source": "server.toml",
        "line": 1,
        "column": 12,
        "start": 11,
        "end": 13
      },
      "help": "Set hostname to a reachable address, e.g. \"127.0.0.1\"."
    }
  ]
}
```

## narrow

The narrow example shows the ready-made narrowing helpers that convert spec integers to the widths a runtime type needs.

```shell
cargo run -q -p confval --example narrow --features derive,color,toml
```

The valid run narrows a port to `u16`, a connection count to `u32`, converts second counts to `Duration`, and passes an absent optional field through as `None`.
The failing run shows a helper reporting an out-of-range value at its span instead of truncating it:

```shell
in range: every helper narrows cleanly
port 8080 accepting 10000 connections, shutdown after 30s
sampling 250 per thousand, request timeout None

out of range: the helper reports at the span instead of truncating
error: value 99999 is out of range for u16
 --> service.toml:1:8
  |
1 | port = 99999
  |        ^^^^^
```

## Additional Examples

Additional examples are available for reference:

- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1).
- Snakeway reverse proxy's [snakeway-conf crate](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src) (advanced usage)
