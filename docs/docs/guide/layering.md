---
sidebar_position: 5
---

# Layering

When an application reads its configuration, the values may come from more than one place.
A file holds the defaults that ship with the project, environment variables set the values a deployment needs, and command line flags override a value for a single run.
Layering combines these sources into one configuration, applying them in order so that a value from a later source overrides the same value from an earlier one.

The `layering` feature assembles the sources for you and produces the same spec type you would parse from a single file.
Environment and command line values are coerced to the type each field declares, and every value keeps its source location, so a configuration error reports the exact file, variable, or flag responsible.

## Enabling Layering

Layering is an opt-in feature.
Add it alongside a format frontend:

```shell
cargo add confval --features "toml,derive,layering"
```

The feature brings in no external crate.

## Concept Overview

This is a high-level look at layering.
The sections that follow cover each part in more detail.

Each configuration source becomes a layer through a provider function.
A file uses `parse_hcl_fields`, `parse_toml_fields`, or `parse_kdl_fields`, the environment uses `env_fields`, and the command line uses `cli_fields`.
You pass the layers to `Assembly` in precedence order and call `assemble` with the spec type you want:

```rust
use confval::layering::{Assembly, cli_fields, env_fields};
use confval::format::toml::parse_toml_fields;
use confval::prelude::*;

let mut sources = SourceMap::new();
let mut report = Report::new();
let base = sources.add("server.toml", file_text);

let spec: Option<ServerSpec> = Assembly::new()
    .merge(parse_toml_fields(&sources, base, &mut report))
    .merge(env_fields(&mut sources, "APP_", &mut report))
    .merge(cli_fields(&mut sources, std::env::args(), &mut report))
    .assemble(&mut report);
```

`assemble` merges the layers and runs the spec's parser once on the result.
The value it returns is the same `ServerSpec` you would get from a single file, so you validate, gate, and lower it exactly as the [pipeline](../pipeline.md) describes.

## Building Layers

Each provider function reads one source and returns a layer.
The file providers read a source you have already registered:

```rust
let file_layer = parse_toml_fields(&sources, base, &mut report);
```

The environment and command line providers register their own sources as they read, so they take the source map by mutable reference:

```rust
let env_layer = env_fields(&mut sources, "APP_", &mut report);
let cli_layer = cli_fields(&mut sources, std::env::args(), &mut report);
```

A provider returns `None` when its source fails to parse, and it records the error in the report.
When any layer is `None`, `assemble` returns `None` before parsing the spec, so check the report for errors after `assemble` as you would after parsing one file.
`assemble` never returns `None` with an empty report.

## Precedence

The call order sets precedence.
`merge` lets a later layer override a value an earlier layer set:

```rust
let spec: Option<ServerSpec> = Assembly::new()
    .merge(file_layer) // base
    .merge(env_layer)  // overrides the file
    .merge(cli_layer)  // overrides the environment
    .assemble(&mut report);
```

`join` lets an earlier layer keep its value and fills only what it did not set.
Use it for a layer of fallback defaults that should not override anything already present:

```rust
let spec: Option<ServerSpec> = Assembly::new()
    .merge(file_layer)
    .join(defaults_layer) // fills gaps only
    .assemble(&mut report);
```

When two layers set the same nested block, the blocks combine field by field.
When two layers set the same array, the higher-precedence array replaces the lower one.

## Environment Variables

`env_fields` reads process environment variables that begin with a prefix.
The prefix is stripped, a double underscore separates nesting levels, a single underscore stays part of a name, and each segment is lowercased:

```rust
let env_layer = env_fields(&mut sources, "APP_", &mut report);
```

With the prefix `APP_`, variables map to fields like this:

- `APP_PORT=8080` sets `port`.
- `APP_SERVER__MAX_BODY_MB=16` sets `server.max_body_mb`.

Prefix matching is case-sensitive and byte-exact, so `app_port` does not match the prefix `APP_`.
Write the prefix exactly as the variables begin, including its trailing underscore.
An empty prefix selects every variable in the process environment.

## Command Line Arguments

`cli_fields` reads flags in the `--key=value` form.
A dot separates nesting levels, and a segment keeps its underscores.
Arguments that are not flags are ignored, so you may pass the whole argument list.
A flag written without its value, such as `--port` for `--port=8080`, is reported as a warning, since the space-separated form sets nothing:

```rust
let cli_layer = cli_fields(&mut sources, std::env::args(), &mut report);
```

Flags map to fields like this:

- `--port=8080` sets `port`.
- `--limits.mode=log` sets `limits.mode`.

## Value Types

A value from an environment variable or a command line flag is always text.
Each value is coerced to the type its field declares, so you write it as a string and the field decides how to read it:

- A field of type `i64` reads `"8080"` as the number `8080`.
- A field of type `String` keeps `"123"` as the text `123`.

A value that does not fit its field is reported as a type error that points at the variable or flag it came from.
For example, `APP_PORT=high` for an integer field reports `expected integer, found string`.

## Unsupported Values

An environment variable or a command line flag sets one value at a path.
Neither can set a list or a repeated block.
A list keeps whatever the file layers provide, so change a list by editing a file.

## Running the Example

The crate ships a `layering` example that assembles one `ServerSpec` from a base file, a joined defaults file, the environment, and the command line:

```shell
cargo run -q -p confval --example layering --features derive,color,toml,layering
```

The base file provides `hostname` and the `limits` block, the environment sets `port` and `limits.mode`, the command line sets `limits.max_body_mb` and `tls`, and the defaults file fills `workers` through `join`:

```shell
listening on 127.0.0.1:9090 with 8 workers
limits: max_body_mb=64 mode=log
tls: true
```
