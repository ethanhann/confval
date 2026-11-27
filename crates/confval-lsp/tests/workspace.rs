//! Integration tests for the workspace pass: a duplicate across documents
//! reported in both, the fix and the close that clear it, today's behavior
//! unchanged without a validator, the empty workspace, unparsed and
//! non-lowering documents, stale reports, the panic guard, deterministic
//! order, cross-file related locations, spanless attribution, and distinct
//! source ids.
#![cfg(feature = "hcl")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Initialized,
    LogMessage, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Initialize, Request as _, Shutdown};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    InitializeParams, InitializedParams, Position, PublishDiagnosticsParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Uri,
    VersionedTextDocumentIdentifier,
};

use confval::prelude::{Located, Report, Validate};
use confval::source::SourceId;
use confval_lsp::{
    Binding, DocumentIndex, Hcl, LspError, Matcher, Router, WorkspaceReport, WorkspaceValidator,
    bind,
};

/// A minimal root spec whose one field is the label `name`, so two documents
/// can collide on it.
#[derive(confval::Spec)]
struct NamedSpec {
    #[confval(label)]
    name: Located<String>,
}

impl Validate for NamedSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// Reports a duplicate `name` on every pair of documents that share one, with
/// a related span pointing at the other document. The iteration order is the
/// index's URI order, so the report is stable across runs.
struct DuplicateName;

impl WorkspaceValidator for DuplicateName {
    fn validate(&self, documents: &DocumentIndex, report: &mut WorkspaceReport) {
        let named: Vec<(&str, &NamedSpec)> = documents
            .iter()
            .filter_map(|(uri, document)| {
                document
                    .spec()
                    .and_then(|spec| spec.downcast_ref::<NamedSpec>())
                    .map(|spec| (uri, spec))
            })
            .collect();
        for (i, (uri, spec)) in named.iter().enumerate() {
            for (other_uri, other) in named.iter().skip(i + 1) {
                if spec.name.value == other.name.value {
                    report
                        .report(uri)
                        .error("duplicate service name")
                        .at(spec.name.span)
                        .related(other.name.span, "first declared here")
                        .emit();
                    report
                        .report(other_uri)
                        .error("duplicate service name")
                        .at(other.name.span)
                        .related(spec.name.span, "first declared here")
                        .emit();
                }
            }
        }
    }
}

/// Reports one spanless error into a document the tests choose.
struct SpanlessError(&'static str);

impl WorkspaceValidator for SpanlessError {
    fn validate(&self, _documents: &DocumentIndex, report: &mut WorkspaceReport) {
        report
            .report(self.0)
            .error("acme needs tls_automation")
            .emit();
    }
}

/// Reports an error into a URI no document holds, beside the duplicates.
struct StaleReporter;

impl WorkspaceValidator for StaleReporter {
    fn validate(&self, documents: &DocumentIndex, report: &mut WorkspaceReport) {
        report.report("file:///ghost.hcl").error("nowhere").emit();
        duplicates(documents, report);
    }
}

/// Reports an error when two open documents share a source id, which would
/// make the span table ambiguous.
struct DistinctSources;

impl WorkspaceValidator for DistinctSources {
    fn validate(&self, documents: &DocumentIndex, report: &mut WorkspaceReport) {
        let entries: Vec<(&str, SourceId)> = documents
            .iter()
            .filter_map(|(uri, document)| document.fields().map(|fields| (uri, fields.source())))
            .collect();
        for (i, (uri, id)) in entries.iter().enumerate() {
            if entries[..i].iter().any(|(_, other)| other == id) {
                report.report(uri).error("shared source id").emit();
            }
        }
    }
}

/// Panics on its first run, then reports the duplicates.
struct PanicsOnce {
    panicked: AtomicBool,
}

impl WorkspaceValidator for PanicsOnce {
    fn validate(&self, documents: &DocumentIndex, report: &mut WorkspaceReport) {
        if !self.panicked.swap(true, Ordering::SeqCst) {
            panic!("validator defect");
        }
        duplicates(documents, report);
    }
}

/// The duplicate rule the reuse validators share, delegated to
/// [`DuplicateName`].
fn duplicates(documents: &DocumentIndex, report: &mut WorkspaceReport) {
    DuplicateName.validate(documents, report);
}

/// The client end of a server running on its own thread.
struct Client {
    connection: Connection,
    handle: JoinHandle<Result<(), LspError>>,
}

impl Client {
    /// Starts a router over the bindings and completes the handshake.
    fn multi(bindings: Vec<Binding>) -> Client {
        let (server_conn, client_conn) = Connection::memory();
        let handle = std::thread::spawn(move || match Router::new(bindings) {
            Ok(router) => router.run(&server_conn),
            Err(error) => panic!("the tests pass a non-empty list: {error}"),
        });
        Client::handshake(client_conn, handle)
    }

    /// Starts a router with a workspace validator attached.
    fn multi_with(bindings: Vec<Binding>, validator: Box<dyn WorkspaceValidator>) -> Client {
        let (server_conn, client_conn) = Connection::memory();
        let handle = std::thread::spawn(move || {
            let router = Router::new(bindings)
                .expect("the tests pass a non-empty list")
                .with_workspace_validator(validator);
            router.run(&server_conn)
        });
        Client::handshake(client_conn, handle)
    }

    fn handshake(connection: Connection, handle: JoinHandle<Result<(), LspError>>) -> Client {
        let mut client = Client { connection, handle };
        client.send(Message::Request(Request::new(
            RequestId::from(0),
            Initialize::METHOD.to_string(),
            InitializeParams::default(),
        )));
        let _initialized: Response = client.recv_response(RequestId::from(0)).1;
        client.send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )));
        client
    }

    /// Ends the session: a shutdown request, an exit notification, and a join
    /// that surfaces the server's exit result.
    fn shutdown(mut self) {
        let _shutdown = self.request(Shutdown::METHOD, serde_json::Value::Null);
        self.send(Message::Notification(Notification::new(
            Exit::METHOD.to_string(),
            serde_json::Value::Null,
        )));
        match self.handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => panic!("the server exited with an error: {error}"),
            Err(_) => panic!("the server thread panicked"),
        }
    }

    fn send(&self, message: Message) {
        if self.connection.sender.send(message).is_err() {
            panic!("the server hung up");
        }
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> Response {
        let id = RequestId::from(1);
        self.send(Message::Request(Request::new(
            id.clone(),
            method.to_string(),
            params,
        )));
        self.recv_response(id).1
    }

    fn recv_response(&mut self, id: RequestId) -> (RequestId, Response) {
        loop {
            match self.connection.receiver.recv() {
                Ok(Message::Response(response)) => {
                    assert_eq!(response.id, id, "out of order response");
                    return (id, response);
                }
                Ok(_) => {}
                Err(_) => panic!("the server closed the connection"),
            }
        }
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

    fn change(&self, uri: &Uri, text: &str) {
        self.send(Message::Notification(Notification::new(
            DidChangeTextDocument::METHOD.to_string(),
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
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

    /// The next published diagnostics, skipping the routing log messages an
    /// open emits.
    fn recv_publish(&self) -> PublishDiagnosticsParams {
        loop {
            match self.connection.receiver.recv().unwrap() {
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

    /// The next published diagnostics when one is waiting, else `None`.
    fn try_recv_publish(&self) -> Option<PublishDiagnosticsParams> {
        loop {
            match self.connection.receiver.try_recv() {
                Ok(Message::Notification(notification))
                    if notification.method == PublishDiagnostics::METHOD =>
                {
                    return Some(serde_json::from_value(notification.params).unwrap());
                }
                Ok(Message::Notification(notification))
                    if notification.method == LogMessage::METHOD => {}
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }
}

/// The two bindings the workspace tests route with.
fn named_bindings() -> Vec<Binding> {
    vec![
        bind::<NamedSpec, Hcl>(Matcher::FileName("a.hcl".to_string()), Hcl),
        bind::<NamedSpec, Hcl>(Matcher::FileName("b.hcl".to_string()), Hcl),
    ]
}

fn uri(name: &str) -> Uri {
    Uri::from_str(&format!("file:///{name}")).unwrap()
}

fn doc(name: &str) -> String {
    format!("name = \"{name}\"\n")
}

#[test]
fn a_duplicate_across_documents_is_reported_in_both() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, &doc("api"));
    let first = client.recv_publish();
    client.open(&b, &doc("api"));
    let in_a = client.recv_publish();
    let in_b = client.recv_publish();

    // Assert
    assert_eq!(first.uri, a);
    assert!(
        first
            .diagnostics
            .iter()
            .all(|d| d.message != "duplicate service name"),
        "one document alone has no duplicate"
    );
    assert_eq!(in_a.uri, a);
    assert_eq!(in_b.uri, b);
    for published in [&in_a, &in_b] {
        assert!(
            published
                .diagnostics
                .iter()
                .any(|d| d.message == "duplicate service name"),
            "the duplicate reports in both, got: {:?}",
            published.diagnostics
        );
    }
    client.shutdown();
}

#[test]
fn fixing_one_document_clears_the_duplicate_in_both() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));
    client.open(&a, &doc("api"));
    client.open(&b, &doc("api"));
    let _open_a = client.recv_publish();
    let _in_a = client.recv_publish();
    let _in_b = client.recv_publish();

    // Act
    client.change(&a, &doc("web"));
    let cleared_a = client.recv_publish();
    let cleared_b = client.recv_publish();

    // Assert
    assert_eq!(cleared_a.uri, a);
    assert_eq!(cleared_b.uri, b);
    for published in [&cleared_a, &cleared_b] {
        assert!(
            published.diagnostics.is_empty(),
            "the fix clears both, got: {:?}",
            published.diagnostics
        );
    }
    client.shutdown();
}

#[test]
fn without_a_validator_only_the_changed_document_publishes() {
    // Arrange
    let client = Client::multi(named_bindings());
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));
    client.open(&a, &doc("api"));
    let _open_a = client.recv_publish();
    client.open(&b, &doc("api"));
    let _open_b = client.recv_publish();

    // Act
    client.change(&a, &doc("web"));

    // Assert
    let changed = client.recv_publish();
    assert_eq!(changed.uri, a);
    assert!(
        client.try_recv_publish().is_none(),
        "the unchanged document does not republish without a validator"
    );
    client.shutdown();
}

#[test]
fn the_empty_workspace_pass_publishes_nothing() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let a = uri("a.hcl");
    client.open(&a, &doc("api"));
    let _open = client.recv_publish();

    // Act
    client.close(&a);

    // Assert
    let clear = client.recv_publish();
    assert_eq!(clear.uri, a);
    assert!(
        clear.diagnostics.is_empty(),
        "the close clears the document"
    );
    assert!(
        client.try_recv_publish().is_none(),
        "the pass over an empty index publishes nothing"
    );
    client.shutdown();
}

#[test]
fn closing_one_duplicate_clears_the_survivor() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));
    client.open(&a, &doc("api"));
    client.open(&b, &doc("api"));
    let _open_a = client.recv_publish();
    let _in_a = client.recv_publish();
    let _in_b = client.recv_publish();

    // Act
    client.close(&a);

    // Assert
    let clear = client.recv_publish();
    assert_eq!(clear.uri, a);
    assert!(
        clear.diagnostics.is_empty(),
        "the close clears the closed file"
    );
    let survivor = client.recv_publish();
    assert_eq!(survivor.uri, b);
    assert!(
        survivor.diagnostics.is_empty(),
        "the surviving file's duplicate clears, got: {:?}",
        survivor.diagnostics
    );
    client.shutdown();
}

#[test]
fn an_unparsed_document_is_skipped_and_the_others_still_publish() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, "= = =");
    let broken = client.recv_publish();
    client.open(&b, &doc("api"));
    let in_a = client.recv_publish();
    let in_b = client.recv_publish();

    // Assert
    assert_eq!(broken.uri, a);
    assert!(
        !broken.diagnostics.is_empty(),
        "the parse errors publish for the broken document"
    );
    assert_eq!(in_a.uri, a);
    assert!(
        in_a.diagnostics
            .iter()
            .all(|d| d.message != "duplicate service name"),
        "a document with no spec contributes no duplicate"
    );
    assert_eq!(in_b.uri, b);
    client.shutdown();
}

#[test]
fn a_report_for_a_uri_no_document_holds_publishes_nowhere() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(StaleReporter));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, &doc("api"));
    let _open_a = client.recv_publish();
    client.open(&b, &doc("api"));
    let in_a = client.recv_publish();
    let in_b = client.recv_publish();

    // Assert
    assert_eq!(in_a.uri, a);
    assert_eq!(in_b.uri, b);
    assert!(
        in_a.diagnostics.iter().all(|d| d.message != "nowhere"),
        "no issue publishes into the stale uri in a"
    );
    assert!(
        in_b.diagnostics.iter().all(|d| d.message != "nowhere"),
        "no issue publishes into the stale uri in b"
    );
    assert!(
        client.try_recv_publish().is_none(),
        "no publish names the uri no document holds"
    );
    client.shutdown();
}

#[test]
fn a_panicking_validator_drops_the_notification_and_the_server_keeps_serving() {
    // Arrange
    let client = Client::multi_with(
        named_bindings(),
        Box::new(PanicsOnce {
            panicked: AtomicBool::new(false),
        }),
    );
    let a = uri("a.hcl");

    // Act
    client.open(&a, &doc("api"));

    // Assert
    assert!(
        client.try_recv_publish().is_none(),
        "the panicked notification drops its publish"
    );

    // Act: a later change runs the validator again and publishes.
    client.change(&a, &doc("web"));
    let recovered = client.recv_publish();

    // Assert
    assert_eq!(recovered.uri, a);
    assert!(
        client.try_recv_publish().is_none(),
        "the server kept serving after the panic"
    );
    client.shutdown();
}

#[test]
fn the_merged_publish_walks_the_documents_in_uri_order() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, &doc("api"));
    let first = client.recv_publish();
    client.open(&b, &doc("api"));
    let second = client.recv_publish();
    let third = client.recv_publish();

    // Assert
    assert_eq!(first.uri, a, "the first open publishes a alone");
    assert_eq!(second.uri, a, "the merged set starts at a");
    assert_eq!(third.uri, b, "the merged set continues in uri order");
    client.shutdown();
}

#[test]
fn a_cross_file_related_span_points_at_the_other_document() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, &doc("api"));
    let _open_a = client.recv_publish();
    client.open(&b, &doc("api"));
    let _in_a = client.recv_publish();
    let in_b = client.recv_publish();

    // Assert
    let duplicate = in_b
        .diagnostics
        .iter()
        .find(|d| d.message == "duplicate service name")
        .expect("the duplicate reports in b");
    let related = duplicate
        .related_information
        .as_ref()
        .expect("the related span");
    assert_eq!(
        related[0].location.uri, a,
        "the first declaration points at a"
    );
    assert_eq!(related[0].message, "first declared here");
    client.shutdown();
}

#[test]
fn a_spanless_workspace_issue_publishes_on_its_attributed_document() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(SpanlessError("file:///a.hcl")));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, &doc("api"));
    let in_a = client.recv_publish();
    client.open(&b, &doc("api"));
    let _in_a = client.recv_publish();
    let in_b = client.recv_publish();

    // Assert
    let spanless = in_a
        .diagnostics
        .iter()
        .find(|d| d.message == "acme needs tls_automation")
        .expect("the spanless issue publishes in a");
    assert_eq!(
        spanless.range.start,
        Position {
            line: 0,
            character: 0
        }
    );
    assert!(
        in_b.diagnostics
            .iter()
            .all(|d| d.message != "acme needs tls_automation"),
        "the spanless issue stays on its attributed document"
    );
    client.shutdown();
}

#[test]
fn a_non_lowering_document_is_skipped_and_the_others_still_publish() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DuplicateName));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, "");
    let lowered_none = client.recv_publish();
    client.open(&b, &doc("api"));
    let _republished_a = client.recv_publish();
    let in_b = client.recv_publish();

    // Assert
    assert_eq!(lowered_none.uri, a);
    assert!(
        lowered_none
            .diagnostics
            .iter()
            .any(|d| d.message.contains("name")),
        "the missing required name reports per document, got: {:?}",
        lowered_none.diagnostics
    );
    assert_eq!(in_b.uri, b);
    assert!(
        in_b.diagnostics
            .iter()
            .all(|d| d.message != "duplicate service name"),
        "a document that does not lower contributes no duplicate"
    );
    client.shutdown();
}

#[test]
fn two_open_documents_carry_distinct_source_ids() {
    // Arrange
    let client = Client::multi_with(named_bindings(), Box::new(DistinctSources));
    let (a, b) = (uri("a.hcl"), uri("b.hcl"));

    // Act
    client.open(&a, &doc("api"));
    let _open_a = client.recv_publish();
    client.open(&b, &doc("api"));
    let in_a = client.recv_publish();
    let in_b = client.recv_publish();

    // Assert
    for published in [&in_a, &in_b] {
        assert!(
            published
                .diagnostics
                .iter()
                .all(|d| d.message != "shared source id"),
            "each document keeps its own source id, got: {:?}",
            published.diagnostics
        );
    }
    client.shutdown();
}
