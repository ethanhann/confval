# confval-lsp

Schema-generic language server core for [`confval`](https://crates.io/crates/confval) configuration files.

A configuration written against a confval spec has a known legal surface.
This crate serves that surface to an editor: diagnostics, completion, hover, navigation, document symbols, and quick fixes.
The core is generic over the root spec and the format frontend, so one core serves an HCL, a TOML, a KDL, a JSON, or a YAML document.

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
