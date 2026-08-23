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
    bind::<DeviceSpec, _>(Matcher::Fn(Box::new(device_matcher)), Hcl),
    bind::<RouteSpec, _>(Matcher::Fn(Box::new(route_matcher)), Hcl),
])
```

Bindings are tried in declaration order, and the first match wins.
`Matcher::FileName` compares the document's file name.
`Matcher::Fn` receives the document's absolute path and answers with your own rule, which can read the same include patterns your loader reads.
`Matcher::Any` accepts every document, so an `Any` binding declared last acts as a fallback.

A matcher must not panic.
When your rule hits a problem, such as an unreadable file, return `false` so the binding declines and the next one is tried.
A panic drops the whole open silently, with no warning log, which is why the rule exists.

A document that matches no binding stays open but inert.
It gets no diagnostics and empty answers, and the server logs a warning naming it, so a mismatch between the editor's file patterns and your bindings is visible rather than silent.

The server routes a document once, when the editor opens it.
If you change the inputs your matchers read, such as an include pattern in the entrypoint, a document that is already open keeps its old schema.
Reopen the file to route it again.

## Embedding the router

Sometimes your host already owns an `lsp-server` connection, such as a test harness over an in-memory pair or a process that speaks LSP over a socket.
`Router` is the server behind both `serve` functions, and you can run it over your own connection.

```rust
use confval_lsp::{Matcher, Router, bind, serve_multi, Hcl};

let router = Router::new(vec![bind::<AppSpec, _>(Matcher::Any, Hcl)])?;
router.run(&connection)?;
```

`Router::new` refuses an empty binding list with an error, before any connection exists.
Both entry points and `Router` return the crate's `LspError`, so a `main` returning `Result<(), confval_lsp::LspError>` propagates everything with the question mark.

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
It binds an entrypoint spec to `gateway.cvm` by file name and a device spec to any `device.*` file through a closure matcher, with no fallback, so any other document shows the unmatched warning.
The sample documents are plain HCL under the made-up `.cvm` extension, so an IDE registers this server on its own file pattern beside a `.hcl` registration.

```
cargo run -p confval-lsp --example serve_multi
```

The documents under `dev/sample_configs/multi/` exercise it: a valid entrypoint, a valid device, a device with a bad keyword and an out-of-range port, and one file no binding matches.

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
