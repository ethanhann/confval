//! One integration test that drives the transport shell without an editor.

mod fixture;

use std::str::FromStr;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidOpenTextDocument, Exit, Initialized, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Completion, Initialize, Request as _, Shutdown};
use lsp_types::{
    CompletionItem, CompletionParams, DidOpenTextDocumentParams, InitializeParams,
    InitializedParams, PartialResultParams, Position, PublishDiagnosticsParams,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Uri,
    WorkDoneProgressParams,
};

use confval_lsp::{Hcl, Server};
use fixture::ServerSpec;

/// Receives messages until one satisfies the predicate, or panics on hangup.
fn recv_until<T>(connection: &Connection, mut pick: impl FnMut(&Message) -> Option<T>) -> T {
    while let Ok(message) = connection.receiver.recv() {
        if let Some(value) = pick(&message) {
            return value;
        }
    }
    panic!("the server closed the connection before the expected message");
}

#[test]
fn the_server_runs_the_initialize_open_and_request_cycle() {
    // Arrange
    let (server, client) = Connection::memory();
    let handle = std::thread::spawn(move || Server::<ServerSpec, Hcl>::new(Hcl).run(&server));
    let uri = Uri::from_str("file:///server.hcl").unwrap();

    // Act, initialize.
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            Initialize::METHOD.to_string(),
            InitializeParams::default(),
        )))
        .unwrap();
    let _init: Response = recv_until(&client, |message| match message {
        Message::Response(response) if response.id == RequestId::from(1) => Some(response.clone()),
        _ => None,
    });
    client
        .sender
        .send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )))
        .unwrap();

    // Act, open a document with an out-of-range port.
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "hcl".to_string(),
                    version: 1,
                    text: "hostname = \"api\"\nport = 99999\n".to_string(),
                },
            },
        )))
        .unwrap();
    let diagnostics: PublishDiagnosticsParams = recv_until(&client, |message| match message {
        Message::Notification(notification)
            if notification.method == PublishDiagnostics::METHOD =>
        {
            Some(serde_json::from_value(notification.params.clone()).unwrap())
        }
        _ => None,
    });

    // Act, request completion in the root body.
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(2),
            Completion::METHOD.to_string(),
            CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position {
                        line: 2,
                        character: 0,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            },
        )))
        .unwrap();
    let completion: Response = recv_until(&client, |message| match message {
        Message::Response(response) if response.id == RequestId::from(2) => Some(response.clone()),
        _ => None,
    });

    // Act, shut down.
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(3),
            Shutdown::METHOD.to_string(),
            (),
        )))
        .unwrap();
    let _shutdown: Response = recv_until(&client, |message| match message {
        Message::Response(response) if response.id == RequestId::from(3) => Some(response.clone()),
        _ => None,
    });
    client
        .sender
        .send(Message::Notification(Notification::new(
            Exit::METHOD.to_string(),
            (),
        )))
        .unwrap();

    // Assert
    assert!(
        !diagnostics.diagnostics.is_empty(),
        "the open document publishes at least one diagnostic"
    );
    let items: Vec<CompletionItem> =
        serde_json::from_value(completion.result.expect("a completion result")).unwrap();
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"workers"),
        "the root body offers an unset field, got: {labels:?}"
    );
    handle.join().unwrap().unwrap();
}
