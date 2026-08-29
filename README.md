# confval

[![Build](https://github.com/ethanhann/confval/actions/workflows/build.yml/badge.svg?branch=main)](https://github.com/ethanhann/confval/actions/workflows/build.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://ethanhann.com/confval/coverage/badge.json)](https://github.com/ethanhann/confval/actions/workflows/build.yml)
[![Tests](https://img.shields.io/endpoint?url=https://ethanhann.com/confval/coverage/tests-badge.json)](https://github.com/ethanhann/confval/actions/workflows/build.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue)](LICENSE)

Configuration parsing, validation, and lowering primitives for Rust.

Configuration is parsed span-first, so every value carries the byte range it came from and every later check can point
at the place in the file that caused it.
The core does not depend on any file format.
A frontend converts one syntax into a format-neutral field model.
Everything after parsing works against that model.

The [toml example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/toml.rs), [hcl example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/hcl.rs),
[kdl example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/kdl.rs),
[json example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/json.rs), and
[yaml example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/yaml.rs) demonstrate how this crate is meant to be used.
The five run the same steps in the same order.
Only the source text, its file name, and the two format calls that parse and emit it differ.

The [layering example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/layering.rs) assembles one configuration from a file, the environment,
and the command line.

The [templates example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/templates.rs) writes an annotated configuration file from the spec types, which runs the pipeline backward.

Sixteen examples ship in total, fourteen in the confval crate and two runnable language servers in confval-lsp.
The [Examples](https://ethanhann.com/confval/docs/examples) page lists what each one covers along with its run command.

See the [confval documentation](https://ethanhann.com/confval/) for the full API overview.

The confval crate was extracted from the [Snakeway reverse proxy](https://snakeway.dev) configuration subsystem.

## Language server

The [confval-lsp](https://github.com/ethanhann/confval/tree/main/crates/confval-lsp) crate is a schema-generic language server core.
It binds to your `#[derive(Spec)]` root and a format frontend.
`serve_multi` serves a configuration that spans several document shapes from one process, one binding per shape.
It serves diagnostics, completion, hover, navigation, rename, document highlight, document symbols, folding, and quick fixes for any of the five formats.
See [Language Server](https://ethanhann.com/confval/docs/guide/language-server) for the walkthrough and a runnable example.

## Usage examples

- [In this repo](https://github.com/ethanhann/confval/tree/main/crates/confval/examples)
- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1)
- Snakeway reverse proxy's [snakeway-conf](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src) crate (advanced usage)

## Agent skills

confval ships a binary that installs two agent skills into a project: one that scaffolds a pipeline and one that keeps its layers in sync as the configuration grows.
Install the binary with `cargo install confval`, then run `confval init`.
See [Agent Skills](https://ethanhann.com/confval/docs/agent-skills) for what each skill does, where the files land, and what the outcomes and exit codes mean.

## License

Apache-2.0

