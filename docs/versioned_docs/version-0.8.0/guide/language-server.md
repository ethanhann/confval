---
sidebar_position: 11
---

# Language Server

`confval-lsp` is a language server.
It gives an editor the completion, hover, diagnostics, and navigation that [Editor Support](./editor-support.md) describes.
The server works for any confval schema and any format confval parses, so you build one server for your own root spec.

This page is for the developer who owns the spec and wants to run a server.

## Running a server for your spec

Add the crate to the program that owns your spec:

```
cargo add confval-lsp
```

Bind the server to your `#[derive(Spec)]` root and a frontend, then run it over stdio.
The `serve` function owns the connection, the initialize handshake, and the request loop.

For example, serve an HCL document written against a `ServerSpec`:

```rust
use confval_lsp::{serve, Hcl};

serve::<ServerSpec, Hcl>(Hcl)
```

The derive supplies everything the server needs, so naming your root spec and its frontend is the whole binding.

## Trying it against an editor

The crate ships a `serve` example bound to a demo spec, so you can point an editor at a running server before you write your own.
Run it and choose a format:

```
cargo run -p confval-lsp --example serve hcl
```

The example serves over stdin and stdout, so an editor's LSP client launches the built binary at `target/debug/examples/serve` and speaks to it.
The demo spec is there to show the feature set, not to deploy.
Your real server names your own root spec.

## Choosing a format

Every format is a cargo feature, and all of them are on by default.
To ship a server for one format, turn the defaults off and enable that format.
The build then carries one parser instead of all five.

```toml
[dependencies]
confval-lsp = { version = "0.8.0", default-features = false, features = ["toml"] }
```

## Position encoding

An editor addresses text by line and character, and the character count uses UTF-16 code units by default.
The server negotiates the encoding at initialization and prefers UTF-8 when the client supports it, so a range over a value with non-ASCII characters stays aligned.
This is automatic and needs no configuration.
