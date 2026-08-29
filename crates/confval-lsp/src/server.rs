//! The transport shell.
//!
//! It wires the pure handlers and a document store into a runnable server.
//! [`Router`] owns a set of [`Binding`]s and routes each document to the first
//! binding whose matcher accepts it when the client opens it, so one process
//! serves every document of a multi document configuration. It runs over an
//! `lsp-server` connection the caller provides and negotiates the position
//! encoding at initialization. It updates the document store on open and
//! change notifications. It answers the completion, hover, code action,
//! navigation, rename, document highlight, symbol, link, and folding requests
//! by calling the handlers. It publishes diagnostics on every open and change.
//! [`serve`] binds one root spec and one frontend over the same router, for a
//! configuration of one document shape.

use std::collections::HashMap;
use std::fmt;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, LogMessage,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentHighlightRequest, DocumentLinkRequest,
    DocumentSymbolRequest, FoldingRangeRequest, GotoDefinition, HoverRequest, PrepareRenameRequest,
    References, Rename, Request as _,
};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    InitializeParams, InitializeResult, LogMessageParams, MessageType, PublishDiagnosticsParams,
    Uri,
};

use confval::format::{Fields, FromFields};
use confval::pipeline::{Validate, ValidateNested};
use confval::schema::ToSchema;

use crate::binding::{Binding, Matcher, bind, file_path};
use crate::capabilities::{
    completion_support, negotiate, server_capabilities, supports_hierarchical_symbols,
};
use crate::encoding::PositionEncoding;
use crate::frontend::Frontend;
use crate::handlers;

mod requests;

/// The error the entry points and [`Router`] return.
///
/// It wraps whatever failed underneath, whether transport, protocol, or the
/// refusal of an empty binding list, and renders it through `Display`. Every
/// error type converts into it, so `?` works inside a function returning it.
/// It does not implement `std::error::Error`, so a `main` returning
/// `anyhow::Result<()>` or `Result<(), Box<dyn Error>>` does not accept it
/// directly.
pub struct LspError(Box<dyn std::error::Error + Send + Sync>);

impl fmt::Display for LspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for LspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<E: Into<Box<dyn std::error::Error + Send + Sync>>> From<E> for LspError {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

/// JSON-RPC error code for an unknown method.
const METHOD_NOT_FOUND: i32 = -32601;
/// JSON-RPC error code for invalid request parameters.
const INVALID_PARAMS: i32 = -32602;
/// JSON-RPC error code for a server-side failure.
const INTERNAL_ERROR: i32 = -32603;

/// One open document: its current text, its current parse, the report that
/// parse produced, and the index of the binding that matched it. The parse is
/// `None` when the text does not parse, and the binding index is `None` when
/// no binding matched. A publish maps the stored report rather than parsing
/// again.
struct Document {
    text: String,
    tree: Option<Fields>,
    report: confval::diagnostic::Report,
    binding: Option<usize>,
}

/// The erased language server: a set of bindings and the document store.
///
/// Routing runs at every open, so reopening a document routes it again, which
/// is the operator's remedy when the routing inputs on disk changed. An
/// unmatched document is held as text but gets no parse, no diagnostics, and
/// empty answers, with one warning log naming it at open.
pub struct Router {
    bindings: Vec<Binding>,
    encoding: PositionEncoding,
    completion_client: handlers::ClientSupport,
    hierarchical: bool,
    documents: HashMap<String, Document>,
}

impl Router {
    /// A router over the given bindings. An empty list is refused, because a
    /// server that can answer for no document is a construction mistake that
    /// should surface before the handshake.
    pub fn new(bindings: Vec<Binding>) -> Result<Self, LspError> {
        if bindings.is_empty() {
            return Err("at least one binding is required".into());
        }
        Ok(Self::over(bindings))
    }

    /// A router over a non-empty binding list. The encoding defaults to
    /// UTF-16, the LSP default, until initialization negotiates it.
    fn over(bindings: Vec<Binding>) -> Self {
        Self {
            bindings,
            encoding: PositionEncoding::Utf16,
            completion_client: handlers::ClientSupport::default(),
            hierarchical: false,
            documents: HashMap::new(),
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
            DocumentLinkRequest::METHOD => {
                respond(request, method, |params| self.document_links(params))
            }
            FoldingRangeRequest::METHOD => {
                respond(request, method, |params| self.folding_ranges(params))
            }
            DocumentHighlightRequest::METHOD => {
                respond(request, method, |params| self.document_highlight(params))
            }
            PrepareRenameRequest::METHOD => {
                respond(request, method, |params| self.prepare_rename(params))
            }
            Rename::METHOD => respond_fallible(request, method, |params| self.rename(params)),
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
                    let matched = self.open_document(&uri, params.text_document.text);
                    log_routing(connection, &uri, self.matched(matched))?;
                    self.publish(connection, &uri)?;
                }
            }
            DidChangeTextDocument::METHOD => {
                if let Ok(params) = notification
                    .extract::<DidChangeTextDocumentParams>(DidChangeTextDocument::METHOD)
                {
                    let uri = params.text_document.uri;
                    if let Some(change) = params.content_changes.into_iter().next_back() {
                        // A change for a document the store does not hold
                        // routes it the way an open would, so a client that
                        // sends a change without an open still gets a routed,
                        // parsed document.
                        if self.documents.contains_key(uri.as_str()) {
                            self.update_document(&uri, change.text);
                        } else {
                            let matched = self.open_document(&uri, change.text);
                            log_routing(connection, &uri, self.matched(matched))?;
                        }
                    }
                    self.publish(connection, &uri)?;
                }
            }
            DidCloseTextDocument::METHOD => {
                if let Ok(params) =
                    notification.extract::<DidCloseTextDocumentParams>(DidCloseTextDocument::METHOD)
                {
                    let uri = params.text_document.uri;
                    self.documents.remove(uri.as_str());
                    self.clear_diagnostics(connection, &uri)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Routes a document and stores its text and parse, answering the matched
    /// binding index. `Matcher::Any` needs no path, so a URI that yields none
    /// still routes to an `Any` binding. The caller logs the outcome, so the
    /// store stays free of the connection.
    fn open_document(&mut self, uri: &Uri, text: String) -> Option<usize> {
        let path = file_path(uri);
        let matched = self
            .bindings
            .iter()
            .position(|binding| binding.matcher.matches(path.as_deref()));
        let (tree, report) = parse_with(&self.bindings, matched, &text);
        self.documents.insert(
            uri.as_str().to_string(),
            Document {
                text,
                tree,
                report,
                binding: matched,
            },
        );
        matched
    }

    /// The matched index paired with its binding, for the routing log.
    fn matched(&self, index: Option<usize>) -> Option<(usize, &Binding)> {
        index.and_then(|index| self.bindings.get(index).map(|binding| (index, binding)))
    }

    /// Stores new text for a document the store already holds, keeping the
    /// binding index its open assigned. An absent document stores nothing.
    fn update_document(&mut self, uri: &Uri, text: String) {
        let Some(entry) = self.documents.get_mut(uri.as_str()) else {
            return;
        };
        let (tree, report) = parse_with(&self.bindings, entry.binding, &text);
        entry.text = text;
        entry.tree = tree;
        entry.report = report;
    }

    /// Publishes the diagnostics for a document. An unmatched document
    /// publishes nothing.
    fn publish(&self, connection: &Connection, uri: &Uri) -> Result<(), LspError> {
        let Some(document) = self.documents.get(uri.as_str()) else {
            return Ok(());
        };
        let Some(binding) = document.binding.and_then(|index| self.bindings.get(index)) else {
            return Ok(());
        };
        let diagnostics = handlers::diagnostics(
            binding.validator,
            &binding.schema,
            document.tree.as_ref(),
            &document.report,
            uri,
            &document.text,
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

    /// Clears a document's diagnostics by publishing an empty list, for a
    /// close, matched or not.
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
}

/// Parses with the matched binding's frontend. An unmatched document is held
/// as text with no tree and an empty report, so resolution recovers nothing
/// and every answer stays empty.
fn parse_with(
    bindings: &[Binding],
    matched: Option<usize>,
    text: &str,
) -> (Option<Fields>, confval::diagnostic::Report) {
    match matched.and_then(|index| bindings.get(index)) {
        Some(binding) => binding.frontend.parse_buffer(text),
        None => (None, confval::diagnostic::Report::new()),
    }
}

/// Sends the routing outcome for an opened document: the matched binding at
/// LOG level, or the unmatched document at WARNING, once per open. The
/// message names the decoded file path when the URI yields one, and the URI
/// otherwise, and the matched line records the winning matcher so an operator
/// reads which rule fired without counting declarations.
fn log_routing(
    connection: &Connection,
    uri: &Uri,
    matched: Option<(usize, &Binding)>,
) -> Result<(), LspError> {
    let name = file_path(uri)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| uri.as_str().to_string());
    let (level, message) = match matched {
        Some((index, binding)) => (
            MessageType::LOG,
            format!("{name} matched binding {index} {:?}", binding.matcher),
        ),
        None => (MessageType::WARNING, format!("no binding matches {name}")),
    };
    log(connection, level, message)
}

/// Sends one `window/logMessage` notification.
fn log(connection: &Connection, typ: MessageType, message: String) -> Result<(), LspError> {
    connection
        .sender
        .send(Message::Notification(Notification::new(
            LogMessage::METHOD.to_string(),
            LogMessageParams { typ, message },
        )))?;
    Ok(())
}

/// Extracts a request's parameters and answers through one handler, or answers
/// the invalid-params error when the parameters do not deserialize. A panic in
/// the handler answers the internal error, so one bad request does not take
/// down the server. This guard needs an unwinding panic runtime. A build with
/// `panic = "abort"` still aborts.
fn respond<P, T>(request: Request, method: String, handle: impl FnOnce(P) -> T) -> Response
where
    P: serde::de::DeserializeOwned,
    T: serde::Serialize,
{
    respond_fallible(request, method, |params| Ok::<T, String>(handle(params)))
}

/// The same guard for a handler that can refuse its input. A refusal answers
/// the invalid-params error with the handler's message. The client puts that
/// message in front of the operator, so a rejected name reads as the reason.
fn respond_fallible<P, T>(
    request: Request,
    method: String,
    handle: impl FnOnce(P) -> Result<T, String>,
) -> Response
where
    P: serde::de::DeserializeOwned,
    T: serde::Serialize,
{
    let id = request.id.clone();
    match request.extract::<P>(&method) {
        Ok((id, params)) => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(params))) {
                Ok(Ok(value)) => Response::new_ok(id, value),
                Ok(Err(message)) => Response::new_err(id, INVALID_PARAMS, message),
                Err(_) => {
                    Response::new_err(id, INTERNAL_ERROR, format!("the {method} handler failed"))
                }
            }
        }
        Err(_) => Response::new_err(id, INVALID_PARAMS, "invalid params".to_string()),
    }
}

/// Runs a one binding server over stdio, the entry point a single shape
/// subcommand binds.
pub fn serve<S, F>(frontend: F) -> Result<(), LspError>
where
    S: FromFields + Validate + ValidateNested + ToSchema + 'static,
    F: Frontend + Send + 'static,
{
    serve_multi(vec![bind::<S, F>(Matcher::Any, frontend)])
}

/// Runs a server over stdio for a set of bindings, one per document shape of
/// a multi document configuration. An empty list is refused before the
/// connection opens.
pub fn serve_multi(bindings: Vec<Binding>) -> Result<(), LspError> {
    let router = Router::new(bindings)?;
    let (connection, io_threads) = Connection::stdio();
    router.run(&connection)?;
    io_threads.join()?;
    Ok(())
}

#[cfg(all(test, feature = "hcl"))]
mod tests {
    #![allow(dead_code)]
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::str::FromStr;

    use std::path::PathBuf;

    use confval::prelude::*;
    use lsp_server::RequestId;
    use lsp_types::{
        CompletionParams, CompletionResponse, DocumentLinkParams, Hover, HoverParams, Position,
        TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        TextDocumentPositionParams, VersionedTextDocumentIdentifier,
    };

    use crate::frontends::Hcl;

    range_constraint!(PORT, i64, min: 1, max: 65535);

    /// A minimal root spec: a required host, a ranged defaulted port, and an
    /// optional path field for the document-link tests.
    #[derive(confval::Spec)]
    struct TestSpec {
        hostname: Located<String>,
        #[confval(default = 8080, range = PORT)]
        port: Located<i64>,
        tls_cert: Option<Located<PathBuf>>,
    }

    impl Validate for TestSpec {
        fn validate(&self, _report: &mut Report) {}
    }

    /// A router over the one binding `serve` would build, paired with an
    /// in-memory client end.
    fn setup() -> (Router, Connection, Connection) {
        let (server_conn, client_conn) = Connection::memory();
        let router = Router::new(vec![bind::<TestSpec, Hcl>(Matcher::Any, Hcl)]).unwrap();
        (router, server_conn, client_conn)
    }

    /// The next published diagnostics on the client end, skipping the routing
    /// log messages an open emits.
    fn recv_diagnostics(client: &Connection) -> PublishDiagnosticsParams {
        loop {
            match client.receiver.recv().unwrap() {
                Message::Notification(notification)
                    if notification.method == PublishDiagnostics::METHOD =>
                {
                    return serde_json::from_value(notification.params).unwrap();
                }
                Message::Notification(notification)
                    if notification.method == LogMessage::METHOD => {}
                other => panic!("expected a diagnostics notification, got {other:?}"),
            }
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
        let (mut router, server_conn, client_conn) = setup();
        client_conn
            .sender
            .send(Message::Response(Response::new_ok(
                RequestId::from(1),
                serde_json::Value::Null,
            )))
            .unwrap();
        drop(client_conn);

        // Act
        let result = router.main_loop(&server_conn);

        // Assert
        assert!(result.is_ok(), "the loop returns Ok when the peer hangs up");
    }

    #[test]
    fn an_empty_binding_list_is_refused() {
        // Arrange, Act
        let refused = Router::new(Vec::new());

        // Assert
        let error = refused.map(|_| ()).unwrap_err();
        assert_eq!(error.to_string(), "at least one binding is required");
    }

    #[test]
    fn an_unknown_request_method_returns_method_not_found() {
        // Arrange
        let (mut router, server_conn, client_conn) = setup();
        let request = Request::new(
            RequestId::from(7),
            "custom/method".to_string(),
            serde_json::Value::Null,
        );

        // Act
        router.on_request(&server_conn, request).unwrap();

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
        let (mut router, server_conn, client_conn) = setup();
        let request = Request::new(RequestId::from(8), Completion::METHOD.to_string(), 42i32);

        // Act
        router.on_request(&server_conn, request).unwrap();

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
        let (mut router, server_conn, client_conn) = setup();
        let request = Request::new(RequestId::from(9), HoverRequest::METHOD.to_string(), 42i32);

        // Act
        router.on_request(&server_conn, request).unwrap();

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
        let (mut router, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///open.hcl").unwrap();
        let notification = open_notification(&uri, "hostname = \"api\"\nport = 99999\n");

        // Act
        router.on_notification(&server_conn, notification).unwrap();

        // Assert
        let published = recv_diagnostics(&client_conn);
        assert_eq!(published.uri, uri);
        assert!(
            !published.diagnostics.is_empty(),
            "the out-of-range port publishes a diagnostic"
        );
        assert!(router.documents.contains_key(uri.as_str()));
    }

    #[test]
    fn changing_a_document_updates_the_store_and_republishes() {
        // Arrange
        let (mut router, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///change.hcl").unwrap();
        router
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
        router.on_notification(&server_conn, change).unwrap();

        // Assert
        let published = recv_diagnostics(&client_conn);
        assert!(
            published.diagnostics.is_empty(),
            "the corrected document has no diagnostics, got: {:?}",
            published.diagnostics
        );
        assert_eq!(
            router.documents.get(uri.as_str()).unwrap().text,
            "hostname = \"api\"\nport = 8080\n"
        );
    }

    #[test]
    fn a_change_for_an_unopened_document_routes_it_like_an_open() {
        // Arrange
        let (mut router, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///unopened.hcl").unwrap();
        let change = Notification::new(
            DidChangeTextDocument::METHOD.to_string(),
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 1,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "hostname = \"api\"\nport = 99999\n".to_string(),
                }],
            },
        );

        // Act
        router.on_notification(&server_conn, change).unwrap();

        // Assert
        let published = recv_diagnostics(&client_conn);
        assert!(
            !published.diagnostics.is_empty(),
            "the never-opened document routes, parses, and diagnoses"
        );
        let stored = router.documents.get(uri.as_str()).unwrap();
        assert_eq!(stored.binding, Some(0));
    }

    #[test]
    fn closing_a_document_removes_it_and_clears_diagnostics() {
        // Arrange
        let (mut router, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///close.hcl").unwrap();
        router
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
        router.on_notification(&server_conn, close).unwrap();

        // Assert
        let published = recv_diagnostics(&client_conn);
        assert!(
            published.diagnostics.is_empty(),
            "closing clears the document's diagnostics"
        );
        assert!(!router.documents.contains_key(uri.as_str()));
    }

    #[test]
    fn an_unmatched_document_is_held_as_text_without_a_parse() {
        // Arrange
        let (server_conn, client_conn) = Connection::memory();
        let mut router = Router::new(vec![bind::<TestSpec, Hcl>(
            Matcher::FileName("only.hcl".to_string()),
            Hcl,
        )])
        .unwrap();
        let uri = Uri::from_str("file:///other.hcl").unwrap();
        let notification = open_notification(&uri, "hostname = \"api\"\n");

        // Act
        router.on_notification(&server_conn, notification).unwrap();

        // Assert
        let stored = router.documents.get(uri.as_str()).unwrap();
        assert_eq!(stored.text, "hostname = \"api\"\n");
        assert!(stored.tree.is_none(), "an unmatched document is not parsed");
        assert_eq!(stored.binding, None);
        match client_conn.receiver.recv().unwrap() {
            Message::Notification(notification) => {
                assert_eq!(notification.method, LogMessage::METHOD);
            }
            other => panic!("expected the warning log, got {other:?}"),
        }
        assert!(
            client_conn.receiver.try_recv().is_err(),
            "an unmatched open publishes nothing"
        );
    }

    #[test]
    fn updating_an_absent_document_stores_nothing() {
        // Arrange
        let (mut router, _server_conn, _client_conn) = setup();
        let uri = Uri::from_str("file:///absent.hcl").unwrap();

        // Act
        router.update_document(&uri, "hostname = \"api\"\n".to_string());

        // Assert
        assert!(!router.documents.contains_key(uri.as_str()));
    }

    #[test]
    fn an_unknown_notification_method_is_ignored() {
        // Arrange
        let (mut router, server_conn, client_conn) = setup();
        let notification =
            Notification::new("custom/notification".to_string(), serde_json::Value::Null);

        // Act
        router.on_notification(&server_conn, notification).unwrap();

        // Assert
        assert!(
            client_conn.receiver.try_recv().is_err(),
            "an unhandled notification publishes nothing"
        );
    }

    #[test]
    fn publishing_for_an_unknown_document_sends_nothing() {
        // Arrange
        let (router, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///absent.hcl").unwrap();

        // Act
        router.publish(&server_conn, &uri).unwrap();

        // Assert
        assert!(
            client_conn.receiver.try_recv().is_err(),
            "an absent document publishes nothing"
        );
    }

    #[test]
    fn completion_for_an_unknown_document_is_an_empty_array() {
        // Arrange
        let (router, _server_conn, _client_conn) = setup();
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
        let response = router.completion(params);

        // Assert
        match response {
            CompletionResponse::Array(items) => assert!(items.is_empty()),
            other => panic!("expected an empty array, got {other:?}"),
        }
    }

    #[test]
    fn rename_for_an_unknown_document_is_none() {
        // Arrange
        let (router, _server_conn, _client_conn) = setup();
        let uri = Uri::from_str("file:///absent.hcl").unwrap();
        let params = lsp_types::RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 0,
                    character: 0,
                },
            },
            new_name: "x".to_string(),
            work_done_progress_params: Default::default(),
        };

        // Act
        let result = router.rename(params);

        // Assert
        assert!(
            matches!(result, Ok(None)),
            "an absent document renames nothing"
        );
    }

    #[test]
    fn folding_ranges_for_an_unknown_document_is_empty() {
        // Arrange
        let (router, _server_conn, _client_conn) = setup();
        let uri = Uri::from_str("file:///absent.hcl").unwrap();
        let params = lsp_types::FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        // Act
        let ranges = router.folding_ranges(params);

        // Assert
        assert!(ranges.is_empty(), "an absent document folds nothing");
    }

    #[test]
    fn document_links_for_an_unknown_document_is_empty() {
        // Arrange
        let (router, _server_conn, _client_conn) = setup();
        let uri = Uri::from_str("file:///absent.hcl").unwrap();
        let params = DocumentLinkParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        // Act
        let links = router.document_links(params);

        // Assert
        assert!(links.is_empty());
    }

    #[test]
    fn document_links_returns_a_link_for_a_path_field() {
        // Arrange
        let (mut router, server_conn, client_conn) = setup();
        let uri = Uri::from_str("file:///home/user/server.hcl").unwrap();
        let text = "hostname = \"api\"\ntls_cert = \"/etc/cert.pem\"\n";
        router
            .on_notification(&server_conn, open_notification(&uri, text))
            .unwrap();
        let _diagnostics = recv_diagnostics(&client_conn);
        let params = DocumentLinkParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        // Act
        let links = router.document_links(params);

        // Assert
        assert_eq!(links.len(), 1);
        let target = links[0].target.as_ref().unwrap().as_str();
        assert!(target.ends_with("/etc/cert.pem"), "got: {target}");
    }
}
