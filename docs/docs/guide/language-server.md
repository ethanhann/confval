---
sidebar_position: 11
---

# Language Server

`confval-lsp` is a language server.
It gives an editor the completion, hover, diagnostics, and navigation that [Editor Support](./editor-support.md) describes.
The server works for any confval schema and any format confval parses, so you build one server for your own configuration.

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

The derive supplies everything the server needs, so the binding is only your root spec and its frontend.

## Serving a multi document configuration

Sometimes one configuration spans several document shapes.
An entrypoint file names the top level, and included files carry their own shapes, each with its own root spec.
With `serve` alone, each shape needs its own server process and its own editor registration.

`serve_multi` serves every shape from one process.
You declare one binding per shape with `bind`, pairing a matcher with the shape's root spec and its frontend.
The server picks the schema per document when the editor opens it.

For example, serve an entrypoint by file name and route every other document through your own rule:

```rust
use confval_lsp::{Matcher, bind, serve_multi, Hcl};

serve_multi(vec![
    bind::<AppSpec, _>(Matcher::FileName("app.hcl".into()), Hcl),
    bind::<MiddlewareSpec, _>(Matcher::Fn(Box::new(middleware_matcher)), Hcl),
    bind::<RouteSpec, _>(Matcher::Fn(Box::new(route_matcher)), Hcl),
])
```

Bindings are tried in declaration order, and the first match wins.
`Matcher::FileName` compares the document's file name.
`Matcher::Fn` calls your own rule with the document's absolute path.
The rule can read the same include patterns your loader reads.
`Matcher::Any` accepts every document, so an `Any` binding declared last acts as a fallback.

A matcher must not panic.
When your rule hits a problem, such as an unreadable file, return `false`.
The binding declines, and the server tries the next one.

If the rule panics instead, the server survives, but the file it was routing gets nothing.
No diagnostics appear, every request answers empty, and the log carries no warning.
The file looks ignored, with nothing to say why.
A build with `panic = "abort"` stops the whole server instead.

A document that matches no binding stays open but inert.
It gets no diagnostics, and every request answers empty.
The server logs one warning naming the file.
A mismatch between the editor's file patterns and your bindings therefore shows up in the log instead of passing silently.

The server routes a document once, when the editor opens it.
If you change the inputs your matchers read, such as an include pattern in the entrypoint, a document that is already open keeps its old schema.
Reopen the file to route it again.

## Embedding the router

Sometimes your host already owns an `lsp-server` connection, such as a test harness over an in-memory pair or a process that speaks LSP over a socket.
`Router` is the server behind both `serve` functions, and you can run it over your own connection.

```rust
use confval_lsp::{Matcher, Router, bind, Hcl};

let router = Router::new(vec![bind::<AppSpec, _>(Matcher::Any, Hcl)])?;
router.run(&connection)?;
```

`Router::new` refuses an empty binding list with an error, before any connection exists.
`serve`, `serve_multi`, and `Router` all return the crate's `LspError`.
Give your `main` the return type `Result<(), confval_lsp::LspError>`, and the question mark propagates everything.

## Trying it against an editor

The crate ships a `serve` example bound to a demo spec, so you can point an editor at a running server before you write your own.
Run it and choose a format:

```
cargo run -p confval-lsp --example serve hcl
```

The example serves over stdin and stdout, so an editor's LSP client launches the built binary at `target/debug/examples/serve` and speaks to it.
The demo spec is there to show the feature set, not to deploy.
Your real server names your own root spec.

A second example, `serve_multi`, shows routing.
It binds an entrypoint spec to `gateway.cvm` by file name, and a middleware spec to any `middleware.*` file through a closure matcher.
There is no fallback binding, so any other document shows the unmatched warning.
The sample documents are plain HCL under the made-up `.cvm` extension.
The extension lets an IDE register this server on its own file pattern, beside an existing `.hcl` registration.

```
cargo run -p confval-lsp --example serve_multi
```

The documents under `dev/sample_configs/multi/` exercise it: a valid entrypoint, a valid middleware, a middleware with a bad keyword and an out-of-range port, and one file no binding matches.

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
