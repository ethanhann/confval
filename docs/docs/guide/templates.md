---
sidebar_position: 6
---

# Templates

A spec type encodes the whole configuration surface.
It names every field, holds every default, and carries the doc comment you wrote on each field.
Once you have a spec, you can turn it into a configuration file by running [parsing](./parsing.md) backward.

`to_fields` produces a plain configuration file with every default filled in.
`to_template` produces the same file with each field's documentation rendered as a comment above it.
An operator who opens the file learns what every setting means.

Either method is useful when you build a CLI command that writes a starter config, or when you want to show what the spec resolved to after its defaults were applied.

The crate ships a `templates` example that parses a two-line config and emits it back as an annotated TOML template.
Run it with:

```shell
cargo run -q -p confval --example templates --features derive,color,toml
```

The source sets only `hostname` and `port`.
Populate fills `workers` and `tls` from their defaults and fills the whole `limits` block.
Emit renders each field's comment above it:

```toml
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


## Generating a template

`to_template` produces an annotated template.
`#[derive(Spec)]` generates it.
The prelude exports the `ToFields` trait that declares it.
`spec.to_template()` works wherever the prelude is in scope.

It returns a `Fields`, the same format-neutral field model a frontend produces when it parses a file.
The ordinary emit functions render it.

```rust
use confval::format::toml::{emit_toml, parse_toml};

let spec: ServerSpec = parse_toml(&sources, id, &mut report).unwrap();
let template = emit_toml(&spec.to_template())?;
```

`emit_hcl` renders the same model as HCL.
`emit_kdl` renders it as KDL, with each comment as a `//` line above its node:

```rust
use confval::format::kdl::{emit_kdl, parse_kdl};

let spec: ServerSpec = parse_kdl(&sources, id, &mut report).unwrap();
let template = emit_kdl(&spec.to_template())?;
```

`emit_yaml` renders it as YAML, with each comment as a `#` line above its entry.

JSON has no comment syntax.
`emit_json` renders no doc comments and skips commented entries.
`emit_json(&spec.to_template())` therefore produces the same text as `emit_json(&spec.to_fields())`.
A commented entry stands for a field the source does not set.
The emitted JSON holds every value the spec carries and shows none of the settings the operator has not written.
Use HCL, TOML, KDL, or YAML when you want an annotated template.

A comment is indented to line up with the field it documents.
A comment inside a block is at the block's indentation:

```hcl
# The address the server binds to.
hostname = "127.0.0.1"
# The port the server listens on.
port = 8080
# The number of worker threads.
workers = 4
# Whether TLS is enabled.
tls = false
# Request size and mode limits.
limits {
  # The maximum request body size, in megabytes.
  max_body_mb = 16
  # How limit violations are handled.
  mode = "enforce"
}
```

TOML content is flat.
In a TOML template every comment is at column zero.

Emit writes canonical text rather than rewriting a file a person authored.
It drops the comments and layout the field model never held.
A nested struct is written as a `[table]` in TOML, a block in HCL, or a children node in KDL.

## Writing the comments

A field's comment comes from its Rust doc comment.
You write the documentation once and it serves both the code and the template.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    /// The port the server listens on.
    port: Located<i64>,
    /// Request size and mode limits.
    #[confval(nested, default)]
    limits: Option<Located<LimitsSpec>>,
}
```

A multi-line doc comment renders as one `#` line per source line.
A blank line inside the comment renders as a bare `#`.
When the template text should read differently from the rustdoc, set it on the field with `#[confval(doc = "...")]`.
That text is used in place of the doc comment.

## The plain dump

When you want the configuration file without any commentary, call `to_fields` instead.
It returns the same populated `Fields` as `to_template` but with no comments attached.
The emitted file is a clean dump of values.

```rust
let text = emit_toml(&spec.to_fields())?;
```

The two methods share one populated model and differ only in the comment lines.
The plain dump and the annotated template describe the same configuration.
A spec with a handwritten `ToFields` builds that model with `FieldsBuilder`, described in [Writing emitters by hand](./parsing.md#writing-emitters-by-hand).
Because the model is the same `Fields` type parsing produces, anything that reads a parsed field model reads a populated one the same way.

```toml
hostname = "127.0.0.1"
port = 8080
workers = 4
tls = false

[limits]
max_body_mb = 16
mode = "enforce"
```

## What gets filled

Populate emits an active field only when there is a value to show.
The rules follow from what the parser leaves in the spec:

- A required field is always present and always emitted.
- A leaf with an attribute default is emitted with that default, because parsing already filled it when the source omitted the field.
- A repeated block is emitted once per element.
- An optional block is filled only when you mark it.
  See [Marking Optional Blocks](#marking-optional-blocks).

A block that is present is populated in turn.
A block you wrote but left partial gains its own absent defaults.
A block that is filled is populated to full depth.
One call at the root resolves a nested tree of defaults all the way down.

## Commented-out entries

An absent optional field still exists in the spec.
A template that hid it would leave you unaware the setting is available.
`to_template` renders each one as a commented-out entry instead, with its doc comment above it.
The template documents every field the spec accepts while activating only the ones that carry a value.
`to_fields`, the plain dump, emits no commented entries.

Each shape renders a placeholder you overwrite when uncommenting:

- An optional leaf with no default shows a zero value for its type: the empty string, `0`, `0.0`, or `false`.
- An optional string list shows an empty list.
- An unmarked optional block shows an empty block.
- An empty repeated block shows one empty element.

The marker is each format's own.
TOML and HCL prefix every line with a spaceless `#`.
An entry stays distinguishable from a `# ` doc comment.
Uncommenting is deleting that one character:

```toml
# The PID file path.
#pid_file = ""

#[[svc]]
```

```hcl
# The PID file path.
#pid_file = ""

#svc {
#}
```

YAML uses the same spaceless `#`, with the marker after the indentation.
Deleting it leaves the entry at its own column:

```yaml
# The PID file path.
#pid_file: ""

#svc:
  #- {}
```

An empty repeated block shows one `#- {}` element, because uncommenting must leave an empty instance of the right shape.
A bare `- ` would read as a null element.

KDL uses its native slashdash, a disabled node the parser reads and discards.
Uncommenting is deleting the `/-`:

```kdl
// The PID file path.
/-pid_file ""
```

A commented entry is invisible to every parser.
A template parses to the same configuration with or without its commented entries.

## Marking optional blocks

An optional block is absent until you write it.
Parsing keeps an absent `Option<Located<S>>` as `None`.
The spec stays faithful to what the operator wrote.
Populate has to know which absent blocks to fill and which to leave out, because a block the runtime never applies should not appear in a populated view.

The `#[confval(nested, default)]` marker on an optional nested field is that signal.
A marked block is filled from its type's `Default` when you populate.
An unmarked optional block is left absent.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
    #[confval(nested, default)]
    limits: Option<Located<LimitsSpec>>,
    #[confval(nested)]
    telemetry: Option<Located<TelemetrySpec>>,
}
```

Here `limits` is filled from `LimitsSpec::default()` and `telemetry` is left absent.
A marked block requires its inner type to implement `Default`.
Deriving that with [`#[confval(derive_default)]`](./parsing.md#deriving-default-from-the-attribute-defaults) generates the impl from the same attribute defaults.
One declaration drives parsing, `Default`, and the populated output together.
The marker changes populate only.
Parsing still leaves an absent block `None` and the read path is unchanged.

## When emit fails

Emit returns a `Result`, because not every field model can be written faithfully in every format.

| Format | Populated spec fails when | Notes |
|--------|---------------------------|-------|
| TOML | Never | TOML has a literal for every value populate produces and quotes any key. |
| KDL | Never | KDL writes every populate value. |
| YAML | Never | YAML 1.2 writes infinity and NaN natively, and any key writes as a quoted string. |
| JSON | A float default is infinity or NaN | JSON has no literal for non-finite floats. |
| HCL | A float default is infinity or NaN | HCL has no keyword for non-finite floats. |

If your defaults are ordinary numbers, template generation cannot fail.
You can `expect` on the emit call.

[Format Limitations](./format-limitations.md) collects every format's gaps in one place.

Emit can also fail on a `Fields` that a frontend parsed rather than populated, because a parsed model can carry a name or a value the target format cannot write.
A value with no representation, such as a TOML datetime, fails in any format.
A name that is not a valid identifier fails when you emit HCL, which has no way to quote it.
TOML and KDL quote it without trouble.
HCL writes a value and a block side by side under one name.
A TOML key names one thing.
`emit_toml` refuses that pair rather than dropping one of the two.
Neither format can write one name twice for plain values.
Both emitters refuse that as well.
`emit_json` and `emit_yaml` group a repeated name into one member holding a sequence.
A name used twice for plain values emits to both.
Both refuse a value beside a same-named block.
The only way either can write that pair is a duplicate key, which loses one of the two members.
Each emit error names the dotted path of the field responsible.
A failure in a large tree points at its location.

A tree assembled by layering can carry unparsed text from an environment variable or a command line flag.
That text emits as a string literal, since its type was never decided.
A typed reparse of the emitted file therefore reads those leaves as strings.

## Detached spans and the fixed point

Every value populate produces carries a detached span, a span with no source location.
The value comes from the spec and not from a file.
A parsed value points at the bytes it came from.
A populated value has no bytes to point at.
Its span records only that the value was filled.

Equality on a `Located` value ignores the span.
A populated spec compares equal by value to the same spec parsed back from its own emitted output.
Populate only fills defaults.
Running it on a spec that is already complete adds nothing.
Populate is therefore a fixed point.
Populate a spec, emit it, parse it back, and populate again.
The result is the same configuration.
