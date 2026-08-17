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
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, References,
    Request as _,
};
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams, Hover, HoverParams,
    InitializeParams, InitializeResult, PublishDiagnosticsParams, ReferenceParams, Uri,
};

use confval::format::{Fields, FromFields};
use confval::pipeline::{Validate, ValidateNested};
use confval::schema::{Schema, ToSchema};

use crate::capabilities::{
    completion_support, negotiate, server_capabilities, supports_hierarchical_symbols,
};
use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::Frontend;
use crate::handlers;

/// The boxed error the transport propagates.
type LspError = Box<dyn std::error::Error + Send + Sync>;

/// JSON-RPC error code for an unknown method.
const METHOD_NOT_FOUND: i32 = -32601;
/// JSON-RPC error code for invalid request parameters.
const INVALID_PARAMS: i32 = -32602;
/// JSON-RPC error code for a server-side failure.
const INTERNAL_ERROR: i32 = -32603;

/// One open document: its current text, its current parse, `None` when the
/// text does not parse, and the report that parse produced, so a publish maps
/// it rather than parsing again.
struct Document {
    text: String,
    tree: Option<Fields>,
    report: confval::diagnostic::Report,
}

/// The language server, generic over the root spec and the frontend.
pub struct Server<S, F> {
    frontend: F,
    encoding: PositionEncoding,
    completion_client: handlers::ClientSupport,
    hierarchical: bool,
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
            completion_client: handlers::ClientSupport::default(),
            hierarchical: false,
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
        self.completion_client = completion_support(&params);
        self.hierarchical = supports_hierarchical_symbols(&params);
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
                    // The same guard as `respond`: a panic while updating a
                    // document or publishing diagnostics drops that
                    // notification instead of taking down the server.
                    if let Ok(result) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            self.on_notification(connection, notification)
                        }))
                    {
                        result?;
                    }
                }
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    /// Dispatches a request to its handler.
    fn on_request(&mut self, connection: &Connection, request: Request) -> Result<(), LspError> {
        let id = request.id.clone();
        let method = request.method.clone();
        let response = match method.as_str() {
            Completion::METHOD => respond(request, method, |params| self.completion(params)),
            HoverRequest::METHOD => respond(request, method, |params| self.hover(params)),
            GotoDefinition::METHOD => respond(request, method, |params| self.definition(params)),
            References::METHOD => respond(request, method, |params| self.references(params)),
            DocumentSymbolRequest::METHOD => {
                respond(request, method, |params| self.document_symbols(params))
            }
            CodeActionRequest::METHOD => {
                respond(request, method, |params| self.code_action(params))
            }
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

    /// Stores a document's text, its current parse, which is `None` when the
    /// text does not parse, and the parse report. Resolution recovers from the
    /// raw text in that case, so a stale tree is never kept.
    fn set_document(&mut self, uri: &Uri, text: String) {
        let (tree, report) = self.frontend.parse_buffer(&text);
        let entry = self.documents.entry(key(uri)).or_insert_with(|| Document {
            text: String::new(),
            tree: None,
            report: confval::diagnostic::Report::new(),
        });
        entry.text = text;
        entry.tree = tree;
        entry.report = report;
    }

    /// Publishes the diagnostics for a document.
    fn publish(&mut self, connection: &Connection, uri: &Uri) -> Result<(), LspError> {
        let Some(document) = self.documents.get(&key(uri)) else {
            return Ok(());
        };
        let diagnostics = handlers::diagnostics::<S>(
            &self.schema,
            document.tree.as_ref(),
            &document.report,
            &document.text,
            uri,
            self.encoding,
        );
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
        let Some((document, index, context)) = self.resolve_at(uri, position) else {
            return CompletionResponse::Array(Vec::new());
        };
        let items = handlers::completion(
            &self.frontend,
            &handlers::Cx {
                schema: &self.schema,
                fields: document.tree.as_ref(),
                ctx: &context,
                text: &document.text,
            },
            &index,
            self.encoding,
            self.completion_client,
        );
        CompletionResponse::Array(items)
    }

    /// Resolves the cursor of a positioned request against a stored document.
    fn resolve_at(
        &self,
        uri: &Uri,
        position: lsp_types::Position,
    ) -> Option<(&Document, LineIndex, crate::frontend::CursorContext)> {
        let document = self.documents.get(&key(uri))?;
        let index = LineIndex::new(&document.text);
        let offset = index.offset_of(&document.text, position, self.encoding);
        let context = self
            .frontend
            .resolve(document.tree.as_ref(), &document.text, offset);
        Some((document, index, context))
    }

    /// Computes the definition response for a request.
    fn definition(&self, params: GotoDefinitionParams) -> Option<lsp_types::Location> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let (document, index, context) = self.resolve_at(uri, position)?;
        handlers::definition(
            &self.schema,
            &context,
            uri,
            &document.text,
            &index,
            self.encoding,
        )
    }

    /// Computes the references response for a request.
    fn references(&self, params: ReferenceParams) -> Vec<lsp_types::Location> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, index, context)) = self.resolve_at(uri, position) else {
            return Vec::new();
        };
        handlers::references(
            &self.schema,
            &context,
            params.context.include_declaration,
            uri,
            &document.text,
            &index,
            self.encoding,
        )
    }

    /// Computes the document-symbol response for a request. A buffer that does
    /// not parse answers nothing, because the outline reads parsed spans.
    fn document_symbols(&self, params: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let uri = &params.text_document.uri;
        let document = self.documents.get(&key(uri))?;
        let tree = document.tree.as_ref()?;
        let index = LineIndex::new(&document.text);
        Some(handlers::document_symbols(
            &self.schema,
            tree,
            handlers::SymbolShape {
                covers_body: self.frontend.block_span_covers_body(),
                hierarchical: self.hierarchical,
            },
            uri,
            &document.text,
            &index,
            self.encoding,
        ))
    }

    /// Computes the code-action response for a request, resolved at the
    /// request range's start.
    fn code_action(&self, params: CodeActionParams) -> Vec<CodeActionOrCommand> {
        let uri = &params.text_document.uri;
        let Some((document, index, context)) = self.resolve_at(uri, params.range.start) else {
            return Vec::new();
        };
        handlers::code_action(
            &self.frontend,
            &handlers::Cx {
                schema: &self.schema,
                fields: document.tree.as_ref(),
                ctx: &context,
                text: &document.text,
            },
            &params.context.diagnostics,
            params.context.only.as_deref(),
            uri,
            &index,
            self.encoding,
        )
    }

    /// Computes the hover response for a request.
    fn hover(&self, params: HoverParams) -> Option<Hover> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let (document, index, context) = self.resolve_at(uri, position)?;
        handlers::hover(
            &handlers::Cx {
                schema: &self.schema,
                fields: document.tree.as_ref(),
                ctx: &context,
                text: &document.text,
            },
            &index,
            self.encoding,
        )
    }
}

/// Extracts a request's parameters and answers through one handler, or answers
/// the invalid-params error when the parameters do not deserialize. A panic in
/// the handler answers the internal error, so one bad request does not take
/// down the server. This guard needs an unwinding panic runtime; a build with
/// `panic = "abort"` still aborts.
fn respond<P, T>(request: Request, method: String, handle: impl FnOnce(P) -> T) -> Response
where
    P: serde::de::DeserializeOwned,
    T: serde::Serialize,
{
    let id = request.id.clone();
    match request.extract::<P>(&method) {
        Ok((id, params)) => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(params))) {
                Ok(value) => Response::new_ok(id, value),
                Err(_) => {
                    Response::new_err(id, INTERNAL_ERROR, format!("the {method} handler failed"))
                }
            }
        }
        Err(_) => Response::new_err(id, INVALID_PARAMS, "invalid params".to_string()),
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

#[cfg(all(test, feature = "hcl"))]
mod tests {
    #![allow(dead_code)]
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::str::FromStr;

    use confval::prelude::*;
    use lsp_server::RequestId;
    use lsp_types::{
        Position, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        TextDocumentPositionParams, VersionedTextDocumentIdentifier,
    };

    use crate::frontends::Hcl;

    range_constraint!(PORT, i64, min: 1, max: 65535);

    /// A minimal root spec: a required host and a ranged, defaulted port.
    #[derive(confval::Spec)]
    struct TestSpec {
        hostname: Located<String>,
        #[confval(default = 8080, range = PORT)]
        port: Located<i64>,
    }

    impl Validate for TestSpec {
        fn validate(&self, _report: &mut Report) {}
    }

    /// A server bound to the HCL frontend, paired with an in-memory client end.
    fn setup() -> (Server<TestSpec, Hcl>, Connection, Connection) {
        let (server_conn, client_conn) = Connection::memory();
        (Server::<TestSpec, Hcl>::new(Hcl), server_conn, client_conn)
    }

    /// The next published diagnostics on the client end.
    fn recv_diagnostics(client: &Connection) -> PublishDiagnosticsParams {
        match client.receiver.recv().unwrap() {
            Message::Notification(notification)
                if notification.method == PublishDiagnostics::METHOD =>
            {
                serde_json::from_value(notification.params).unwrap()
            }
            other => panic!("expected a diagnostics notification, got {other:?}"),
        }
    }

    /// An HCL open notification for a URI and text.
    fn open_notification(uri: &Uri, text: &str) -> Notification {
        Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "hcl".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            },
        )
    }

    #[test]
    fn the_loop_ignores_a_response_and_stops_when_the_connection_closes() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        client_conn
            .sender
            .send(Message::Response(Response::new_ok(
                RequestId::from(1),
                serde_json::Value::Null,
            )))
            .unwrap();
        drop(client_conn);

        // Act
        let result = server.main_loop(&server_conn);

        // Assert
        assert!(result.is_ok(), "the loop returns Ok when the peer hangs up");
    }

    #[test]
    fn an_unknown_request_method_returns_method_not_found() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let request = Request::new(
            RequestId::from(7),
            "custom/method".to_string(),
            serde_json::Value::Null,
        );

        // Act
        server.on_request(&server_conn, request).unwrap();

        // Assert
        let response = match client_conn.receiver.recv().unwrap() {
            Message::Response(response) => response,
            other => panic!("expected a response, got {other:?}"),
        };
        assert_eq!(response.id, RequestId::from(7));
        assert_eq!(response.response_result.unwrap_err().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn a_completion_request_with_invalid_params_returns_invalid_params() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let request = Request::new(RequestId::from(8), Completion::METHOD.to_string(), 42i32);

        // Act
        server.on_request(&server_conn, request).unwrap();

        // Assert
        let response = match client_conn.receiver.recv().unwrap() {
            Message::Response(response) => response,
            other => panic!("expected a response, got {other:?}"),
        };
        assert_eq!(response.id, RequestId::from(8));
        assert_eq!(response.response_result.unwrap_err().code, INVALID_PARAMS);
    }

    #[test]
    fn a_hover_request_with_invalid_params_returns_invalid_params() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let request = Request::new(RequestId::from(9), HoverRequest::METHOD.to_string(), 42i32);

        // Act
        server.on_request(&server_conn, request).unwrap();

        // Assert
        let response = match client_conn.receiver.recv().unwrap() {
            Message::Response(response) => response,
            other => panic!("expected a response, got {other:?}"),
        };
        assert_eq!(response.id, RequestId::from(9));
        assert_eq!(response.response_result.unwrap_err().code, INVALID_PARAMS);
    }

    #[test]
    fn a_panicking_handler_answers_an_internal_error_instead_of_dying() {
        // Arrange
        let request = Request::new(
            RequestId::from(10),
            HoverRequest::METHOD.to_string(),
            serde_json::to_value(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: Uri::from_str("file:///panic.hcl").unwrap(),
                    },
                    position: Position {
                        line: 0,
                        character: 0,
                    },
                },
                work_done_progress_params: Default::default(),
            })
            .unwrap(),
        );

        // Act
        let response = respond(
            request,
            HoverRequest::METHOD.to_string(),
            |_: HoverParams| -> Option<Hover> { panic!("handler defect") },
        );

        // Assert
        assert_eq!(response.id, RequestId::from(10));
        assert_eq!(response.response_result.unwrap_err().code, INTERNAL_ERROR);
    }

    #[test]
    fn opening_a_document_stores_it_and_publishes_diagnostics() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///open.hcl").unwrap();
        let notification = open_notification(&uri, "hostname = \"api\"\nport = 99999\n");

        // Act
        server.on_notification(&server_conn, notification).unwrap();

        // Assert
        let published = recv_diagnostics(&client_conn);
        assert_eq!(published.uri, uri);
        assert!(
            !published.diagnostics.is_empty(),
            "the out-of-range port publishes a diagnostic"
        );
        assert!(server.documents.contains_key(&key(&uri)));
    }

    #[test]
    fn changing_a_document_updates_the_store_and_republishes() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///change.hcl").unwrap();
        server
            .on_notification(
                &server_conn,
                open_notification(&uri, "hostname = \"api\"\nport = 99999\n"),
            )
            .unwrap();
        let _open = recv_diagnostics(&client_conn);
        let change = Notification::new(
            DidChangeTextDocument::METHOD.to_string(),
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "hostname = \"api\"\nport = 8080\n".to_string(),
                }],
            },
        );

        // Act
        server.on_notification(&server_conn, change).unwrap();

        // Assert
        let published = recv_diagnostics(&client_conn);
        assert!(
            published.diagnostics.is_empty(),
            "the corrected document has no diagnostics, got: {:?}",
            published.diagnostics
        );
        assert_eq!(
            server.documents.get(&key(&uri)).unwrap().text,
            "hostname = \"api\"\nport = 8080\n"
        );
    }

    #[test]
    fn closing_a_document_removes_it_and_clears_diagnostics() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///close.hcl").unwrap();
        server
            .on_notification(
                &server_conn,
                open_notification(&uri, "hostname = \"api\"\nport = 99999\n"),
            )
            .unwrap();
        let _open = recv_diagnostics(&client_conn);
        let close = Notification::new(
            DidCloseTextDocument::METHOD.to_string(),
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        );

        // Act
        server.on_notification(&server_conn, close).unwrap();

        // Assert
        let published = recv_diagnostics(&client_conn);
        assert!(
            published.diagnostics.is_empty(),
            "closing clears the document's diagnostics"
        );
        assert!(!server.documents.contains_key(&key(&uri)));
    }

    #[test]
    fn an_unknown_notification_method_is_ignored() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let notification =
            Notification::new("custom/notification".to_string(), serde_json::Value::Null);

        // Act
        server.on_notification(&server_conn, notification).unwrap();

        // Assert
        assert!(
            client_conn.receiver.try_recv().is_err(),
            "an unhandled notification publishes nothing"
        );
    }

    #[test]
    fn publishing_for_an_unknown_document_sends_nothing() {
        // Arrange
        let (mut server, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///absent.hcl").unwrap();

        // Act
        server.publish(&server_conn, &uri).unwrap();

        // Assert
        assert!(
            client_conn.receiver.try_recv().is_err(),
            "an absent document publishes nothing"
        );
    }

    #[test]
    fn completion_for_an_unknown_document_is_an_empty_array() {
        // Arrange
        let (server, _server_conn, _client_conn) = setup();
        let uri = Uri::from_str("file:///absent.hcl").unwrap();
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 0,
                    character: 0,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        // Act
        let response = server.completion(params);

        // Assert
        match response {
            CompletionResponse::Array(items) => assert!(items.is_empty()),
            other => panic!("expected an empty array, got {other:?}"),
        }
    }
}
