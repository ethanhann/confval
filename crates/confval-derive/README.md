# confval-derive

Derive macros for [`confval`](https://crates.io/crates/confval).

This crate provides two procedural macros that remove the boilerplate from a span-first config pipeline:

1. `#[derive(Spec)]` writes the code that **parses** a struct out of a config file.
2. `#[derive(Config)]` writes the code that **lowers** a parsed spec into the runtime form.

See the [confval documentation](https://ethanhann.com/confval) for the full API overview and the design of the
span-first pipeline.

## License

Apache-2.0
