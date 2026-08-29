# confval-lsp

Schema-generic language server core for [`confval`](https://crates.io/crates/confval) configuration files.

A configuration written against a confval spec has a known legal surface.
This crate serves that surface to an editor: diagnostics, completion, hover, navigation, rename, document highlight, document symbols, folding, and quick fixes.
`serve` binds one root spec and one format frontend.
`serve_multi` serves a multi document configuration from one process, with one binding per document shape.
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

A multi document configuration declares one binding per shape.
The server picks the schema per document from its path:

```rust
use confval_lsp::{Matcher, bind, serve_multi, Hcl};

serve_multi(vec![
    bind::<GatewaySpec, _>(Matcher::FileName("gateway.hcl".into()), Hcl),
    bind::<MiddlewareSpec, _>(Matcher::Fn(Box::new(middleware_matcher)), Hcl),
])
```

See the [language server guide](https://ethanhann.com/confval/docs/guide/language-server) for the full walkthrough.
