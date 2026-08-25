//! A repeatable profiling run over the multi binding router, for hotpath.
//!
//! `serve_multi` serves the same bindings over stdio, where an editor drives
//! the traffic and the session never ends on its own. This example drives the
//! same [`Router`] over an in-memory connection instead, replays a fixed set
//! of requests, and shuts the server down, so the run ends and hotpath prints
//! its report.
//!
//! The bindings match `serve_multi`: the first serves `gateway.cvm` by file
//! name, the second serves any `middleware.*` file through a closure matcher.
//! The documents come from `dev/sample_configs/multi/`.
//!
//! Run it with: just profile-lsp

use std::path::PathBuf;
use std::str::FromStr;
use std::thread;

use confval::prelude::*;
use confval_lsp::{Hcl, LspError, Matcher, Router, bind};
use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::Uri;
use serde_json::json;

/// How many times the request block repeats. Raise it for a longer sample.
const ROUNDS: i32 = 200;

range_constraint!(PORT, i64, min: 1, max: 65535);

keyword_enum!(MiddlewareKind, {
    Auth    => "auth",
    Cache   => "cache",
    Logging => "logging",
});

/// The demo entrypoint, served for `gateway.cvm`.
#[derive(confval::Spec)]
struct GatewaySpec {
    /// The address the gateway binds.
    hostname: Located<String>,
    /// The TCP port the gateway listens on.
    #[confval(range = PORT)]
    port: Located<i64>,
    /// Path to the TLS certificate file.
    tls_cert: Option<Located<PathBuf>>,
}

/// A demo middleware document, served for any `middleware.*` file.
#[derive(confval::Spec)]
struct MiddlewareSpec {
    /// The middleware name.
    name: Located<String>,
    /// What the middleware does.
    #[confval(keywords = MiddlewareKind)]
    kind: Located<String>,
    /// The TCP port the middleware answers on.
    #[confval(range = PORT)]
    port: Located<i64>,
}

impl Validate for GatewaySpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MiddlewareSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// One document of the replayed session: where it lives and what it holds.
struct Document {
    uri: String,
    text: String,
}

impl Document {
    fn load(name: &str) -> Result<Self, LspError> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../dev/sample_configs/multi")
            .join(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let uri = Uri::from_str(&format!("file://{}", path.display()))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(Self {
            uri: uri.to_string(),
            text,
        })
    }
}

/// The client end of the session. It sends the handshake, replays the request
/// block once per round, and ends with a shutdown so the router stops.
fn drive(client: &Connection, gateway: &Document, middleware: &Document) -> Result<(), LspError> {
    let mut id = 0;
    let mut request = |method: &str, params: serde_json::Value| -> Result<(), LspError> {
        id += 1;
        client.sender.send(Message::Request(Request::new(
            RequestId::from(id),
            method.to_string(),
            params,
        )))?;
        Ok(())
    };

    request("initialize", json!({"processId": null, "capabilities": {}}))?;
    notify(client, "initialized", json!({}))?;

    for round in 1..=ROUNDS {
        for document in [gateway, middleware] {
            notify(
                client,
                "textDocument/didOpen",
                json!({"textDocument": {
                    "uri": document.uri,
                    "languageId": "hcl",
                    "version": round,
                    "text": document.text,
                }}),
            )?;
        }

        let doc = json!({"uri": gateway.uri});
        let at = json!({"line": 1, "character": 4});
        request(
            "textDocument/completion",
            json!({"textDocument": doc, "position": at}),
        )?;
        request(
            "textDocument/hover",
            json!({"textDocument": doc, "position": at}),
        )?;
        request(
            "textDocument/definition",
            json!({"textDocument": doc, "position": at}),
        )?;
        request(
            "textDocument/references",
            json!({"textDocument": doc, "position": at, "context": {"includeDeclaration": true}}),
        )?;
        request("textDocument/documentSymbol", json!({"textDocument": doc}))?;
        request("textDocument/documentLink", json!({"textDocument": doc}))?;
        request(
            "textDocument/codeAction",
            json!({
                "textDocument": doc,
                "range": {"start": at, "end": at},
                "context": {"diagnostics": []},
            }),
        )?;

        notify(
            client,
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": gateway.uri, "version": round + ROUNDS},
                "contentChanges": [{"text": gateway.text}],
            }),
        )?;

        while client.receiver.try_recv().is_ok() {}
    }

    request("shutdown", json!(null))?;
    notify(client, "exit", json!(null))?;
    // Stay connected until the router drops its end. The shutdown response
    // arrives after the exit notification, and a client that hangs up first
    // turns that response into a send error on the server.
    while client.receiver.recv().is_ok() {}
    Ok(())
}

/// Sends one notification to the server end.
fn notify(client: &Connection, method: &str, params: serde_json::Value) -> Result<(), LspError> {
    client.sender.send(Message::Notification(Notification::new(
        method.to_string(),
        params,
    )))?;
    Ok(())
}

#[hotpath::main]
fn main() -> Result<(), LspError> {
    let gateway = Document::load("gateway.cvm")?;
    let middleware = Document::load("middleware.core.cvm")?;
    let (server_conn, client_conn) = Connection::memory();
    let router = Router::new(vec![
        bind::<GatewaySpec, _>(Matcher::FileName("gateway.cvm".to_string()), Hcl),
        bind::<MiddlewareSpec, _>(
            Matcher::Fn(Box::new(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("middleware."))
            })),
            Hcl,
        ),
    ])?;

    let client = thread::spawn(move || drive(&client_conn, &gateway, &middleware));
    router.run(&server_conn)?;
    drop(server_conn);
    client.join().map_err(|_| "the client thread panicked")??;
    Ok(())
}
