//! The transport shell.
//!
//! It wires the pure handlers and a document store into a runnable server,
//! generic over the root spec `S` and the frontend `F`. It owns the `lsp-server`
//! connection, negotiates the position encoding at initialization, updates the
//! document store on open and change notifications, and answers the completion,
//! hover, and diagnostic requests by calling the handlers.

use std::collections::HashMap;
use std::marker::PhantomData;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{Completion, HoverRequest, Request as _};
use lsp_types::{
    CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverParams, InitializeParams, InitializeResult,
    PublishDiagnosticsParams, Uri,
};

use confval::format::{Fields, FromFields};
use confval::pipeline::{Validate, ValidateNested};
use confval::schema::{Schema, ToSchema};

use crate::capabilities::{negotiate, server_capabilities};
use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::Frontend;
use crate::handlers;

/// The boxed error the transport propagates.
type LspError = Box<dyn std::error::Error + Send + Sync>;

/// JSON-RPC error code for an unknown method.
const METHOD_NOT_FOUND: i32 = -32601;
/// JSON-RPC error code for invalid request parameters.
const INVALID_PARAMS: i32 = -32602;

/// One open document: its current text and its current parse, `None` when the
/// text does not parse.
struct Document {
    text: String,
    tree: Option<Fields>,
}

/// The language server, generic over the root spec and the frontend.
pub struct Server<S, F> {
    frontend: F,
    encoding: PositionEncoding,
    schema: Schema,
    documents: HashMap<String, Document>,
    spec: PhantomData<fn() -> S>,
}

impl<S, F> Server<S, F>
where
    S: FromFields + Validate + ValidateNested + ToSchema,
    F: Frontend,
{
    /// A server bound to a frontend. The encoding defaults to UTF-16, the LSP
    /// default, until initialization negotiates it.
    pub fn new(frontend: F) -> Self {
        Self {
            frontend,
            encoding: PositionEncoding::Utf16,
            schema: S::schema(),
            documents: HashMap::new(),
            spec: PhantomData,
        }
    }

    /// Runs the initialize handshake and the request loop over a connection.
    pub fn run(mut self, connection: &Connection) -> Result<(), LspError> {
        let (id, params) = connection.initialize_start()?;
        let params: InitializeParams = serde_json::from_value(params)?;
        self.encoding = negotiate(&params);
        let result = InitializeResult {
            capabilities: server_capabilities(self.encoding),
            server_info: None,
        };
        connection.initialize_finish(id, serde_json::to_value(result)?)?;
        self.main_loop(connection)
    }

    /// The message loop. It returns when the client sends a shutdown and exit.
    fn main_loop(&mut self, connection: &Connection) -> Result<(), LspError> {
        for message in &connection.receiver {
            match message {
                Message::Request(request) => {
                    if connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    self.on_request(connection, request)?;
                }
                Message::Notification(notification) => {
                    self.on_notification(connection, notification)?;
                }
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    /// Dispatches a completion or hover request.
    fn on_request(&mut self, connection: &Connection, request: Request) -> Result<(), LspError> {
        let id = request.id.clone();
        let method = request.method.clone();
        let response = match method.as_str() {
            Completion::METHOD => match request.extract::<CompletionParams>(Completion::METHOD) {
                Ok((id, params)) => Response::new_ok(id, self.completion(params)),
                Err(_) => Response::new_err(id, INVALID_PARAMS, "invalid params".to_string()),
            },
            HoverRequest::METHOD => match request.extract::<HoverParams>(HoverRequest::METHOD) {
                Ok((id, params)) => Response::new_ok(id, self.hover(params)),
                Err(_) => Response::new_err(id, INVALID_PARAMS, "invalid params".to_string()),
            },
            _ => Response::new_err(id, METHOD_NOT_FOUND, format!("unhandled method: {method}")),
        };
        connection.sender.send(Message::Response(response))?;
        Ok(())
    }

    /// Dispatches an open or change notification, then publishes diagnostics.
    fn on_notification(
        &mut self,
        connection: &Connection,
        notification: Notification,
    ) -> Result<(), LspError> {
        let method = notification.method.clone();
        match method.as_str() {
            DidOpenTextDocument::METHOD => {
                if let Ok(params) =
                    notification.extract::<DidOpenTextDocumentParams>(DidOpenTextDocument::METHOD)
                {
                    let uri = params.text_document.uri;
                    self.set_document(&uri, params.text_document.text);
                    self.publish(connection, &uri)?;
                }
            }
            DidChangeTextDocument::METHOD => {
                if let Ok(params) = notification
                    .extract::<DidChangeTextDocumentParams>(DidChangeTextDocument::METHOD)
                {
                    let uri = params.text_document.uri;
                    if let Some(change) = params.content_changes.into_iter().next_back() {
                        self.set_document(&uri, change.text);
                    }
                    self.publish(connection, &uri)?;
                }
            }
            DidCloseTextDocument::METHOD => {
                if let Ok(params) =
                    notification.extract::<DidCloseTextDocumentParams>(DidCloseTextDocument::METHOD)
                {
                    let uri = params.text_document.uri;
                    self.documents.remove(&key(&uri));
                    self.clear_diagnostics(connection, &uri)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Stores a document's text and its current parse, which is `None` when the
    /// text does not parse. Resolution recovers from the raw text in that case,
    /// so a stale tree is never kept.
    fn set_document(&mut self, uri: &Uri, text: String) {
        let tree = self.frontend.parse_tree(&text);
        let entry = self.documents.entry(key(uri)).or_insert_with(|| Document {
            text: String::new(),
            tree: None,
        });
        entry.text = text;
        entry.tree = tree;
    }

    /// Publishes the diagnostics for a document.
    fn publish(&mut self, connection: &Connection, uri: &Uri) -> Result<(), LspError> {
        let Some(document) = self.documents.get(&key(uri)) else {
            return Ok(());
        };
        let diagnostics =
            handlers::diagnostics::<S, F>(&self.frontend, &document.text, uri, self.encoding);
        let params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version: None,
        };
        connection
            .sender
            .send(Message::Notification(Notification::new(
                PublishDiagnostics::METHOD.to_string(),
                params,
            )))?;
        Ok(())
    }

    /// Clears a document's diagnostics by publishing an empty list, for a close.
    fn clear_diagnostics(&self, connection: &Connection, uri: &Uri) -> Result<(), LspError> {
        connection
            .sender
            .send(Message::Notification(Notification::new(
                PublishDiagnostics::METHOD.to_string(),
                PublishDiagnosticsParams {
                    uri: uri.clone(),
                    diagnostics: Vec::new(),
                    version: None,
                },
            )))?;
        Ok(())
    }

    /// Computes the completion response for a request.
    fn completion(&self, params: CompletionParams) -> CompletionResponse {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some(document) = self.documents.get(&key(uri)) else {
            return CompletionResponse::Array(Vec::new());
        };
        let index = LineIndex::new(&document.text);
        let offset = index.offset_of(&document.text, position, self.encoding);
        let context = self
            .frontend
            .resolve(document.tree.as_ref(), &document.text, offset);
        let items = handlers::completion(
            &self.frontend,
            &self.schema,
            document.tree.as_ref(),
            &context,
            &document.text,
            &index,
            self.encoding,
        );
        CompletionResponse::Array(items)
    }

    /// Computes the hover response for a request.
    fn hover(&self, params: HoverParams) -> Option<Hover> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let document = self.documents.get(&key(uri))?;
        let index = LineIndex::new(&document.text);
        let offset = index.offset_of(&document.text, position, self.encoding);
        let context = self
            .frontend
            .resolve(document.tree.as_ref(), &document.text, offset);
        handlers::hover(
            &self.schema,
            document.tree.as_ref(),
            &context,
            &document.text,
            &index,
            self.encoding,
        )
    }
}

/// Runs the server over stdio, the entry point a subcommand binds.
pub fn serve<S, F>(frontend: F) -> Result<(), LspError>
where
    S: FromFields + Validate + ValidateNested + ToSchema,
    F: Frontend,
{
    let (connection, io_threads) = Connection::stdio();
    Server::<S, F>::new(frontend).run(&connection)?;
    io_threads.join()?;
    Ok(())
}

/// The document-store key for a URI.
fn key(uri: &Uri) -> String {
    uri.as_str().to_string()
}
