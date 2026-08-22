//! Integration tests for the multi binding router: per-document routing, the
//! declaration-order rule, the unmatched contract, the reopen and change
//! semantics, the matcher panic guard, and the pin that a one binding server
//! still serves a document with no file path.
#![cfg(feature = "hcl")]

mod fixture;

use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Initialized, LogMessage,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Completion, HoverRequest, Initialize, Request as _};
use lsp_types::{
    CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializedParams, LogMessageParams, MessageType,
    Position, PublishDiagnosticsParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier,
};

use confval_lsp::{Binding, Hcl, Matcher, Router, Server, bind};
use fixture::{GatewaySpec, RelaySpec, ServerSpec};

/// The client end of a server running on its own thread.
struct Client {
    connection: Connection,
    next_id: i32,
}

impl Client {
    fn send(&self, message: Message) {
        if self.connection.sender.send(message).is_err() {
            panic!("the server hung up");
        }
    }

    fn recv(&self) -> Message {
        match self.connection.receiver.recv() {
            Ok(message) => message,
            Err(_) => panic!("the server closed the connection"),
        }
    }

    /// Starts a router over the bindings and completes the handshake.
    fn multi(bindings: Vec<Binding>) -> Client {
        let (server_conn, client_conn) = Connection::memory();
        std::thread::spawn(move || match Router::new(bindings) {
            Ok(router) => router.run(&server_conn),
            Err(error) => panic!("the tests pass a non-empty list: {error}"),
        });
        Client::handshake(client_conn)
    }

    /// Starts the one binding `Server` the way `serve` builds it.
    fn single() -> Client {
        let (server_conn, client_conn) = Connection::memory();
        std::thread::spawn(move || Server::<ServerSpec, Hcl>::new(Hcl).run(&server_conn));
        Client::handshake(client_conn)
    }

    fn handshake(connection: Connection) -> Client {
        let client = Client {
            connection,
            next_id: 1,
        };
        client.send(Message::Request(Request::new(
            RequestId::from(0),
            Initialize::METHOD.to_string(),
            InitializeParams::default(),
        )));
        let mut client = client;
        let _initialized: Response = client.recv_response(RequestId::from(0)).1;
        client.send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )));
        client
    }

    fn open(&self, uri: &Uri, text: &str) {
        self.send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: String::new(),
                    version: 1,
                    text: text.to_string(),
                },
            },
        )));
    }

    fn change(&self, uri: &Uri, text: &str, version: i32) {
        self.send(Message::Notification(Notification::new(
            DidChangeTextDocument::METHOD.to_string(),
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_string(),
                }],
            },
        )));
    }

    fn close(&self, uri: &Uri) {
        self.send(Message::Notification(Notification::new(
            DidCloseTextDocument::METHOD.to_string(),
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        )));
    }

    /// Sends a request and returns the notifications received before its
    /// response, beside the response.
    fn request<P: serde::Serialize>(
        &mut self,
        method: &str,
        params: P,
    ) -> (Vec<Notification>, Response) {
        let id = RequestId::from(self.next_id);
        self.next_id += 1;
        self.send(Message::Request(Request::new(
            id.clone(),
            method.to_string(),
            params,
        )));
        self.recv_response(id)
    }

    fn recv_response(&mut self, id: RequestId) -> (Vec<Notification>, Response) {
        let mut seen = Vec::new();
        loop {
            match self.recv() {
                Message::Response(response) if response.id == id => return (seen, response),
                Message::Notification(notification) => seen.push(notification),
                other => panic!("expected a response or a notification, got {other:?}"),
            }
        }
    }

    /// The next published diagnostics, skipping the routing log messages.
    fn recv_diagnostics(&self) -> PublishDiagnosticsParams {
        loop {
            match self.recv() {
                Message::Notification(notification)
                    if notification.method == PublishDiagnostics::METHOD =>
                {
                    return parse(notification.params);
                }
                Message::Notification(notification)
                    if notification.method == LogMessage::METHOD => {}
                other => panic!("expected a diagnostics notification, got {other:?}"),
            }
        }
    }

    /// The next message, which the caller expects to be a log message.
    fn recv_log(&self) -> LogMessageParams {
        match self.recv() {
            Message::Notification(notification) if notification.method == LogMessage::METHOD => {
                parse(notification.params)
            }
            other => panic!("expected a log message, got {other:?}"),
        }
    }

    fn completion_at(&mut self, uri: &Uri) -> (Vec<Notification>, Response) {
        self.request(
            Completion::METHOD,
            CompletionParams {
                text_document_position: position_params(uri),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            },
        )
    }
}

fn position_params(uri: &Uri) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position {
            line: 0,
            character: 0,
        },
    }
}

/// A deserialized notification or response payload.
fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
    match serde_json::from_value(value) {
        Ok(parsed) => parsed,
        Err(error) => panic!("the payload deserializes: {error}"),
    }
}

fn uri(text: &str) -> Uri {
    match Uri::from_str(text) {
        Ok(uri) => uri,
        Err(error) => panic!("the test URI parses: {error}"),
    }
}

fn messages(published: &PublishDiagnosticsParams) -> Vec<String> {
    published
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

fn server_binding(name: &str) -> Binding {
    bind::<ServerSpec, Hcl>(Matcher::FileName(name.to_string()), Hcl)
}

fn relay_binding(name: &str) -> Binding {
    bind::<RelaySpec, Hcl>(Matcher::FileName(name.to_string()), Hcl)
}

const SERVER_TEXT: &str = "hostname = \"api\"\nport = 8080\n";
const RELAY_TEXT: &str = "port = 1\n";

#[test]
fn each_document_routes_to_its_own_schema() {
    // Arrange
    let client = Client::multi(vec![
        server_binding("server.hcl"),
        relay_binding("relay.hcl"),
    ]);

    // Act
    client.open(&uri("file:///w/server.hcl"), SERVER_TEXT);
    client.open(&uri("file:///w/relay.hcl"), SERVER_TEXT);

    // Assert
    let server_diags = client.recv_diagnostics();
    assert!(
        server_diags.diagnostics.is_empty(),
        "the text is valid against ServerSpec, got: {:?}",
        messages(&server_diags)
    );
    let relay_diags = client.recv_diagnostics();
    assert!(
        messages(&relay_diags)
            .iter()
            .any(|message| message.contains("unknown field: hostname")),
        "the same text routes to RelaySpec, where hostname is unknown, got: {:?}",
        messages(&relay_diags)
    );
}

#[test]
fn the_first_of_two_overlapping_bindings_wins() {
    // Arrange
    let client = Client::multi(vec![server_binding("dup.hcl"), relay_binding("dup.hcl")]);

    // Act
    client.open(&uri("file:///w/dup.hcl"), RELAY_TEXT);

    // Assert
    let published = client.recv_diagnostics();
    assert!(
        messages(&published)
            .iter()
            .any(|message| message.contains("hostname")),
        "the first binding's ServerSpec reports its missing hostname, got: {:?}",
        messages(&published)
    );
}

#[test]
fn a_change_keeps_the_binding_its_open_assigned() {
    // Arrange
    let flag = Arc::new(AtomicBool::new(false));
    let read = Arc::clone(&flag);
    let client = Client::multi(vec![
        bind::<ServerSpec, Hcl>(
            Matcher::Fn(Box::new(move |_| read.load(Ordering::SeqCst))),
            Hcl,
        ),
        bind::<RelaySpec, Hcl>(Matcher::Any, Hcl),
    ]);
    let document = uri("file:///w/x.hcl");
    client.open(&document, RELAY_TEXT);
    let opened = client.recv_diagnostics();
    assert!(
        opened.diagnostics.is_empty(),
        "the open routes to RelaySpec"
    );
    flag.store(true, Ordering::SeqCst);

    // Act
    client.change(&document, SERVER_TEXT, 2);

    // Assert
    let published = client.recv_diagnostics();
    assert!(
        messages(&published)
            .iter()
            .any(|message| message.contains("unknown field: hostname")),
        "the change stays on RelaySpec although the matcher now accepts, got: {:?}",
        messages(&published)
    );
}

#[test]
fn reopening_a_document_routes_it_again() {
    // Arrange
    let flag = Arc::new(AtomicBool::new(false));
    let read = Arc::clone(&flag);
    let client = Client::multi(vec![
        bind::<ServerSpec, Hcl>(
            Matcher::Fn(Box::new(move |_| read.load(Ordering::SeqCst))),
            Hcl,
        ),
        bind::<RelaySpec, Hcl>(Matcher::Any, Hcl),
    ]);
    let document = uri("file:///w/x.hcl");
    client.open(&document, SERVER_TEXT);
    let first = client.recv_diagnostics();
    assert!(
        messages(&first)
            .iter()
            .any(|message| message.contains("unknown field: hostname")),
        "the first open routes to RelaySpec"
    );
    flag.store(true, Ordering::SeqCst);

    // Act
    client.open(&document, SERVER_TEXT);

    // Assert
    let published = client.recv_diagnostics();
    assert!(
        published.diagnostics.is_empty(),
        "the reopen re-routes to ServerSpec, got: {:?}",
        messages(&published)
    );
}

#[test]
fn an_unmatched_document_is_inert_and_logged() {
    // Arrange
    let mut client = Client::multi(vec![server_binding("only.hcl")]);
    let document = uri("file:///w/other.hcl");

    // Act
    client.open(&document, SERVER_TEXT);

    // Assert
    let log = client.recv_log();
    assert_eq!(log.typ, MessageType::WARNING);
    assert_eq!(log.message, "no binding matches file:///w/other.hcl");
    let (seen, response) = client.completion_at(&document);
    assert!(
        seen.iter()
            .all(|notification| notification.method != PublishDiagnostics::METHOD),
        "the unmatched open publishes no diagnostics"
    );
    let items: CompletionResponse =
        serde_json::from_value(response.response_result.unwrap()).unwrap();
    match items {
        CompletionResponse::Array(items) => {
            assert!(items.is_empty(), "an unmatched document completes nothing");
        }
        other => panic!("expected an empty array, got {other:?}"),
    }
    let (_, hover) = client.request(HoverRequest::METHOD, position_params(&document));
    assert_eq!(
        hover.response_result.unwrap(),
        serde_json::Value::Null,
        "an unmatched document hovers nothing"
    );
    client.close(&document);
    let cleared = client.recv_diagnostics();
    assert!(
        cleared.diagnostics.is_empty(),
        "the close clears with an empty list"
    );
}

#[test]
fn a_matched_open_logs_the_winning_binding() {
    // Arrange
    let client = Client::multi(vec![
        server_binding("server.hcl"),
        relay_binding("relay.hcl"),
    ]);

    // Act
    client.open(&uri("file:///w/relay.hcl"), RELAY_TEXT);

    // Assert
    let log = client.recv_log();
    assert_eq!(log.typ, MessageType::LOG);
    assert_eq!(log.message, "file:///w/relay.hcl matched binding 1");
}

#[test]
fn a_document_with_no_file_path_keeps_full_service_under_one_binding() {
    // Arrange
    let client = Client::single();
    let document = uri("untitled:Untitled-1");

    // Act
    client.open(&document, "hostname = \"api\"\nport = 99999\n");

    // Assert
    let published = client.recv_diagnostics();
    assert!(
        messages(&published)
            .iter()
            .any(|message| message.contains("port")),
        "a never-saved buffer still gets the range diagnostic, got: {:?}",
        messages(&published)
    );
}

#[test]
fn a_panicking_matcher_drops_the_open_and_the_server_answers_on() {
    // Arrange
    let mut client = Client::multi(vec![bind::<ServerSpec, Hcl>(
        Matcher::Fn(Box::new(|_| panic!("matcher defect"))),
        Hcl,
    )]);
    let document = uri("file:///w/x.hcl");

    // Act
    client.open(&document, SERVER_TEXT);

    // Assert
    let (seen, response) = client.completion_at(&document);
    assert!(
        seen.is_empty(),
        "the panicking open publishes nothing, got: {seen:?}"
    );
    let items: CompletionResponse =
        serde_json::from_value(response.response_result.unwrap()).unwrap();
    match items {
        CompletionResponse::Array(items) => {
            assert!(items.is_empty(), "the dropped document answers empty");
        }
        other => panic!("expected an empty array, got {other:?}"),
    }
}

#[test]
fn three_bindings_share_one_frontend_type() {
    // Arrange
    let client = Client::multi(vec![
        server_binding("server.hcl"),
        relay_binding("relay.hcl"),
        bind::<GatewaySpec, Hcl>(Matcher::FileName("gateway.hcl".to_string()), Hcl),
    ]);

    // Act
    client.open(&uri("file:///w/relay.hcl"), RELAY_TEXT);
    client.open(&uri("file:///w/gateway.hcl"), RELAY_TEXT);

    // Assert
    let relay_diags = client.recv_diagnostics();
    assert!(
        relay_diags.diagnostics.is_empty(),
        "the relay document is valid against RelaySpec, got: {:?}",
        messages(&relay_diags)
    );
    let gateway_diags = client.recv_diagnostics();
    assert!(
        messages(&gateway_diags)
            .iter()
            .any(|message| message.contains("unknown field: port")),
        "the gateway document rejects the relay field, got: {:?}",
        messages(&gateway_diags)
    );
}

#[cfg(feature = "yaml")]
#[test]
fn two_bindings_serve_two_formats_over_one_connection() {
    // Arrange
    use confval_lsp::Yaml;
    let client = Client::multi(vec![
        bind::<ServerSpec, Hcl>(Matcher::FileName("app.hcl".to_string()), Hcl),
        bind::<ServerSpec, Yaml>(Matcher::FileName("app.yaml".to_string()), Yaml),
    ]);

    // Act
    client.open(&uri("file:///w/app.yaml"), "hostname: api\nport: 99999\n");
    client.open(
        &uri("file:///w/app.hcl"),
        "hostname = \"api\"\nport = 99999\n",
    );

    // Assert
    let yaml_diags = client.recv_diagnostics();
    assert!(
        messages(&yaml_diags)
            .iter()
            .any(|message| message.contains("port")),
        "the YAML document parses through its own frontend, got: {:?}",
        messages(&yaml_diags)
    );
    let hcl_diags = client.recv_diagnostics();
    assert!(
        messages(&hcl_diags)
            .iter()
            .any(|message| message.contains("port")),
        "the HCL document parses through its own frontend, got: {:?}",
        messages(&hcl_diags)
    );
}

#[test]
fn an_empty_binding_list_is_refused_before_any_connection() {
    // Arrange, Act
    let refused = Router::new(Vec::new());

    // Assert
    let error = refused.map(|_| ()).unwrap_err();
    assert_eq!(error.to_string(), "at least one binding is required");
}
