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
    DidChangeTextDocument, DidOpenTextDocument, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Completion, HoverRequest, Request as _};
use lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, PositionEncodingKind, PublishDiagnosticsParams, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};

use confval::format::{Fields, FromFields};
use confval::pipeline::{Validate, ValidateNested};
use confval::schema::ToSchema;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::Frontend;
use crate::handlers;

/// The boxed error the transport propagates.
type LspError = Box<dyn std::error::Error + Send + Sync>;

/// One open document: its current text and the last field tree that parsed.
struct Document {
    text: String,
    tree: Option<Fields>,
}

/// The language server, generic over the root spec and the frontend.
pub struct Server<S, F> {
    frontend: F,
    encoding: PositionEncoding,
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
        let method = request.method.clone();
        match method.as_str() {
            Completion::METHOD => {
                if let Ok((id, params)) = request.extract::<CompletionParams>(Completion::METHOD) {
                    let response = self.completion(params);
                    connection
                        .sender
                        .send(Message::Response(Response::new_ok(id, response)))?;
                }
            }
            HoverRequest::METHOD => {
                if let Ok((id, params)) = request.extract::<HoverParams>(HoverRequest::METHOD) {
                    let response = self.hover(params);
                    connection
                        .sender
                        .send(Message::Response(Response::new_ok(id, response)))?;
                }
            }
            _ => {
                connection.sender.send(Message::Response(Response::new_ok(
                    request.id,
                    serde_json::Value::Null,
                )))?;
            }
        }
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
            _ => {}
        }
        Ok(())
    }

    /// Stores a document's text and reparses its tree, retaining the last good
    /// tree when the new text does not parse.
    fn set_document(&mut self, uri: &Uri, text: String) {
        let tree = self.frontend.parse_tree(&text);
        let entry = self.documents.entry(key(uri)).or_insert_with(|| Document {
            text: String::new(),
            tree: None,
        });
        entry.text = text;
        if tree.is_some() {
            entry.tree = tree;
        }
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
        let schema = S::schema();
        let items = handlers::completion(
            &self.frontend,
            &schema,
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
        let schema = S::schema();
        handlers::hover(
            &schema,
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

/// Chooses the position encoding from the client's declared support, preferring
/// UTF-8 when the client offers it and falling back to the UTF-16 default.
fn negotiate(params: &InitializeParams) -> PositionEncoding {
    let supported = params
        .capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref());
    match supported {
        Some(kinds) if kinds.contains(&PositionEncodingKind::UTF8) => PositionEncoding::Utf8,
        _ => PositionEncoding::Utf16,
    }
}

/// The server's advertised capabilities.
fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding_kind(encoding)),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions::default()),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    }
}

/// The LSP encoding kind for a negotiated encoding.
fn encoding_kind(encoding: PositionEncoding) -> PositionEncodingKind {
    match encoding {
        PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
        PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
    }
}
