# confval-lsp

Schema-generic language server core for [`confval`](https://crates.io/crates/confval) configuration files.

A configuration written against a confval spec has a known legal surface.
This crate serves that surface to an editor: diagnostics, completion, hover, navigation, document symbols, and quick fixes.
`serve` binds one root spec and one format frontend, and `serve_multi` serves a multi document configuration from one process, one binding per document shape.
One core therefore serves an HCL, TOML, KDL, JSON, or YAML document.

Add it to the program that owns your spec:

```
cargo add confval-lsp
```

Bind the core to your `#[derive(Spec)]` root and a frontend, then run it over stdio:

```rust
use confval_lsp::{serve, Hcl};

serve::<ServerSpec, Hcl>(Hcl)
```

See the [language server guide](https://ethanhann.com/confval/docs/guide/language-server) for the full walkthrough.
