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

The derive supplies everything the server needs, so you name your root spec and its frontend and the server does the rest.

## Serving a multi document configuration

Sometimes one configuration spans several document shapes.
An entrypoint file names the top level, and included files carry their own shapes, each with its own root spec.
With `serve` alone, each shape needs its own server process and its own editor registration.

`serve_multi` serves every shape from one process.
You declare one binding per shape with `bind`, pairing a matcher with the shape's root spec and its frontend, and the server picks the schema per document when the editor opens it.

For example, serve an entrypoint by file name and route every other document through your own rule:

```rust
use confval_lsp::{Matcher, bind, serve_multi, Hcl};

serve_multi(vec![
    bind::<EntrypointSpec, _>(Matcher::FileName("snakeway.hcl".into()), Hcl),
    bind::<DevicesFile, _>(Matcher::Fn(Box::new(devices_matcher)), Hcl),
    bind::<IngressSpec, _>(Matcher::Fn(Box::new(ingresses_matcher)), Hcl),
])
```

Bindings are tried in declaration order, and the first match wins.
`Matcher::FileName` compares the document's file name.
`Matcher::Fn` receives the document's absolute path and answers with your own rule, which can read the same include patterns your loader reads.
`Matcher::Any` accepts every document, so an `Any` binding declared last acts as a fallback.

A matcher must not panic.
When your rule hits a problem, such as an unreadable file, return `false` so the document reports as unmatched.

A document that matches no binding stays open but inert.
It gets no diagnostics and empty answers, and the server logs a warning naming it, so a mismatch between the editor's file patterns and your bindings is visible rather than silent.

The server routes a document once, when the editor opens it.
If you change the inputs your matchers read, such as an include pattern in the entrypoint, a document that is already open keeps its old schema.
Reopen the file to route it again.

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
confval-lsp = { version = "0.9.0", default-features = false, features = ["toml"] }
```

## Position encoding

An editor addresses text by line and character, and the character count uses UTF-16 code units by default.
The server negotiates the encoding at initialization and prefers UTF-8 when the client supports it, so a range over a value with non-ASCII characters stays aligned.
This is automatic and needs no configuration.
