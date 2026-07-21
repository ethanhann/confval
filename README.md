# confval

Configuration parsing, validation, and lowering primitives for Rust.

The [toml example](./crates/confval/examples/toml.rs) and [hcl example](./crates/confval/examples/hcl.rs) demonstrate
how this crate is meant to be used.

See the [confval documentation](https://snakeway.dev/docs/internals/confval/) for the full API overview.

The confval crate was originally extracted from the [Snakeway reverse proxy](https://snakeway.dev) configuration
subsystem after reusable patterns emerged during development.

## Usage examples

- [In this repo](https://github.com/ethanhann/confval/tree/main/crates/confval/examples)
- An example PR for [mini-redis](https://github.com/ethanhann/mini-redis/pull/1)
- Snakeway reverse proxy's [snakeway-conf](https://github.com/snakewayhq/snakeway/tree/main/crates/snakeway-conf/src)
  crate (advanced usage)

## License

Apache-2.0

