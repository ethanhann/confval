# confval

Configuration parsing, validation, and lowering primitives for Rust.

The [toml example](./crates/confval/examples/toml.rs) and [hcl example](./crates/confval/examples/hcl.rs) demonstrate
how this crate is meant to be used.
The [layering example](./crates/confval/examples/layering.rs) assembles one configuration from a file, the environment,
and the command line.

See the [confval documentation](https://ethanhann.com/confval/) for the full API overview.

The confval crate was originally extracted from the [Snakeway reverse proxy](https://snakeway.dev) configuration
subsystem after reusable patterns emerged during development.

## Usage examples

- [In this repo](https://github.com/ethanhann/confval/tree/main/crates/confval/examples)
- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1)
- Snakeway reverse proxy's [snakeway-conf](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src)
  crate (advanced usage)

## License

Apache-2.0

