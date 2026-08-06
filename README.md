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

The [toml example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/toml.rs), [hcl example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/hcl.rs), and
[kdl example](https://github.com/ethanhann/confval/tree/main/crates/confval/examples/kdl.rs) demonstrate how this crate is meant to be used.
The three run the same steps in the same order.
Only the source text, its file name, and the two format calls that parse and emit it differ.
The [layering example](./crates/confval/examples/layering.rs) assembles one configuration from a file, the environment,
and the command line.
The [templates example](./crates/confval/examples/templates.rs) runs the pipeline backward and writes an annotated
configuration file from the spec types.
Twelve examples ship in total.
The [Examples](https://ethanhann.com/confval/docs/examples) page lists what each one covers along with its run
command.

See the [confval documentation](https://ethanhann.com/confval/) for the full API overview.

The confval crate was extracted from the [Snakeway reverse proxy](https://snakeway.dev) configuration subsystem.

## Usage examples

- [In this repo](https://github.com/ethanhann/confval/tree/main/crates/confval/examples)
- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1)
- Snakeway reverse proxy's [snakeway-conf](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src)
  crate (advanced usage)

## License

Apache-2.0

