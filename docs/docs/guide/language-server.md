---
sidebar_position: 11
---

# Language Server

`confval-lsp` is a language server.
It gives an editor the completion, hover, diagnostics, quick fixes, path links, navigation, rename, document highlight, and folding that [Editor Support](./editor-support.md) describes.
The server works for any confval schema and any format confval parses.
You build one server for your own configuration.

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

The derive supplies everything the server needs.
The binding is only your root spec and its frontend.

## Serving a multi document configuration

When one configuration spans several document shapes, an entrypoint file names the top level, and included files have their own shapes, each with its own root spec.
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
If the rule panics, the server survives, but the file it was routing receives no diagnostics and answers every request empty.
A build with `panic = "abort"` stops the whole server instead.

A document that matches no binding stays open but inert.
It receives no diagnostics, and every request answers empty.
The server logs one warning naming the file.
A mismatch between the editor's file patterns and your bindings therefore shows up in the log.

The server routes a document once, when the editor opens it.
If you change the inputs your matchers read, such as an include pattern in the entrypoint, a document that is already open keeps its old schema.
Reopen the file to route it again.

## Validating the workspace

The per-document checks cover one file at a time, so a rule that reads several files cannot run.
An example is a duplicate service name across two include files, or a TLS block that only makes sense beside the entrypoint's settings.
Your loader runs those rules over the whole configuration.
Without a workspace validator, the editor does not check them, so the editor and the CLI disagree on those rules.

A workspace validator lets the server run those rules in the editor.
You implement the trait once, beside the code that owns your specs.

For example, this validator reports a duplicate service name in every file that declares one:

```rust
use confval_lsp::{DocumentIndex, WorkspaceReport, WorkspaceValidator};

struct DuplicateNames;

impl WorkspaceValidator for DuplicateNames {
    fn validate(&self, documents: &DocumentIndex, report: &mut WorkspaceReport) {
        let mut named: Vec<(&str, String)> = Vec::new();
        for (uri, document) in documents.iter() {
            if let Some(spec) = document.spec().and_then(|spec| spec.downcast_ref::<ServiceSpec>()) {
                named.push((uri, spec.name.value.clone()));
            }
        }
        for (i, (uri, name)) in named.iter().enumerate() {
            for (other_uri, other) in named.iter().skip(i + 1) {
                if name == other {
                    report.report(uri).error("duplicate service name").emit();
                    report.report(other_uri).error("duplicate service name").emit();
                }
            }
        }
    }
}
```

`validate` receives every open document the server serves.
Each document exposes its text, its parsed fields, its schema, and, through `spec`, the spec value the server lowered for its own per-document checks.
You downcast that value to your own spec types and run the same checks your loader runs.
Replace `ServiceSpec` with one of your own spec types.
The trait method takes `&self`.
The pass re-runs in full on every open, change, and close.
If your checks are expensive, cache their inputs behind your own interior mutability.
Rebuild from the index on each run.

The validator reports only the checks the per-document pass cannot run.
Do not call `validate_all` or `check_references` inside it.
The per-document pass already runs them.
Attribute every issue to a document when you report it.
The key you pass to `report.report(uri)` determines which file the finding appears in.
Report a cross-file finding in both files it concerns, and attach a related span pointing at the other document.

Pass the validator when you start the server:

```rust
use confval_lsp::{Matcher, bind, serve_multi_with, Hcl};

serve_multi_with(vec![bind::<AppSpec, _>(Matcher::Any, Hcl)], Box::new(DuplicateNames))
```

You can also build a `Router` yourself and call `with_workspace_validator` before `run`.
With no validator registered, a server behaves exactly as before. Only the document you changed publishes.

The workspace pass covers open files only.
For a file that is not open, the CLI remains the check.

## Embedding the router

When your host already owns an `lsp-server` connection, such as a test harness over an in-memory pair or a process that speaks LSP over a socket, you can run the router directly.
`Router` is the server behind both `serve` functions.

```rust
use confval_lsp::{Matcher, Router, bind, Hcl};

let router = Router::new(vec![bind::<AppSpec, _>(Matcher::Any, Hcl)])?;
router.run(&connection)?;
```

`Router::new` refuses an empty binding list with an error, before any connection exists.
`serve`, `serve_multi`, and `Router` all return the crate's `LspError`.
Give your `main` the return type `Result<(), confval_lsp::LspError>`, and the question mark propagates everything.

## Trying it against an editor

The crate ships a `serve` example bound to a demo spec.
You can point an editor at a running server before you write your own.
Run it and choose a format:

```
cargo run -p confval-lsp --example serve hcl
```

The example serves over stdin and stdout.
An editor's LSP client launches the built binary at `target/debug/examples/serve` and speaks to it.
The demo spec shows the feature set, not a production configuration.
Your real server names your own root spec.

A second example, `serve_multi`, shows routing.
It binds an entrypoint spec to `gateway.cvm` by file name, and a middleware spec to any `middleware.*` file through a closure matcher.
There is no fallback binding, so any other document shows the unmatched warning.
The sample documents are plain HCL under the made-up `.cvm` extension.
The extension lets an IDE register this server on its own file pattern, beside an existing `.hcl` registration.
The single-format samples under `dev/sample_configs/` use a made-up extension the same way, one per format.

```
cargo run -p confval-lsp --example serve_multi
```

The documents under `dev/sample_configs/multi/` exercise it: a valid entrypoint, a valid middleware, a middleware with a bad keyword and an out-of-range port, and one file no binding matches.

Both examples accept `--workspace`, which registers a demo workspace validator beside the per-document checks.
With `serve hcl --workspace`, two open files that bind the same `hostname` each report the duplicate.
With `serve_multi --workspace`, two middleware files that declare the same `name` each report the duplicate, and a middleware that listens on the port the gateway binds reports it beside a related span in `gateway.cvm`.

## Choosing a format

Every format is a cargo feature, and all of them are on by default.
To ship a server for one format, turn the defaults off and enable that format.
The build then includes one parser instead of all five.

```toml
[dependencies]
confval-lsp = { version = "0.11", default-features = false, features = ["toml"] }
```

## Position encoding

An editor addresses text by line and character, and the character count uses UTF-16 code units by default.
The server negotiates the encoding at initialization and prefers UTF-8 when the client supports it.
A range over a value with non-ASCII characters stays aligned.
This is automatic and needs no configuration.
