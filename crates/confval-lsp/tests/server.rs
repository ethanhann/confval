//! One integration test that drives the transport shell without an editor.

mod fixture;

use std::str::FromStr;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Initialized,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Completion, HoverRequest, Initialize, Request as _, Shutdown};
use lsp_types::{
    CompletionItem, CompletionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverParams, InitializeParams, InitializedParams,
    PartialResultParams, Position, PublishDiagnosticsParams, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Uri,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};

use confval_lsp::{Hcl, Matcher, Router, Yaml, bind};
use fixture::{GatewaySpec, ServerSpec};

/// The one binding router `serve` runs for the demo spec, over any frontend.
fn single<F: confval_lsp::Frontend + Send + 'static>(frontend: F) -> Router {
    match Router::new(vec![bind::<ServerSpec, F>(Matcher::Any, frontend)]) {
        Ok(router) => router,
        Err(error) => panic!("one binding is never empty: {error}"),
    }
}

/// Sends one request and waits for its response.
fn round_trip(
    client: &lsp_server::Connection,
    id: i32,
    method: impl Into<String>,
    params: impl serde::Serialize,
) -> Response {
    let method: String = method.into();
    if client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(id),
            method.clone(),
            params,
        )))
        .is_err()
    {
        panic!("the server receives the {method} request");
    }
    recv_until(client, |message| match message {
        Message::Response(response) if response.id == RequestId::from(id) => Some(response.clone()),
        _ => None,
    })
}

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
    let handle = std::thread::spawn(move || single(Hcl).run(&server));
    let uri = Uri::from_str("file:///server.hcl").unwrap();

    // Act, initialize.
    let _init = round_trip(&client, 1, Initialize::METHOD, InitializeParams::default());
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
    let completion = round_trip(
        &client,
        2,
        Completion::METHOD,
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
    );

    // Act, request hover on the port value.
    let hover = round_trip(
        &client,
        4,
        HoverRequest::METHOD,
        HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 1,
                    character: 8,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );

    // Act, change the document to a valid one.
    client
        .sender
        .send(Message::Notification(Notification::new(
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
        )))
        .unwrap();
    let changed: PublishDiagnosticsParams = recv_until(&client, |message| match message {
        Message::Notification(notification)
            if notification.method == PublishDiagnostics::METHOD =>
        {
            Some(serde_json::from_value(notification.params.clone()).unwrap())
        }
        _ => None,
    });

    // Act, close the document.
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidCloseTextDocument::METHOD.to_string(),
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        )))
        .unwrap();
    let closed: PublishDiagnosticsParams = recv_until(&client, |message| match message {
        Message::Notification(notification)
            if notification.method == PublishDiagnostics::METHOD =>
        {
            Some(serde_json::from_value(notification.params.clone()).unwrap())
        }
        _ => None,
    });

    // Act, shut down.
    let _shutdown = round_trip(&client, 3, Shutdown::METHOD, ());
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
        serde_json::from_value(completion.response_result.expect("a completion result")).unwrap();
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"workers"),
        "the root body offers an unset field, got: {labels:?}"
    );
    let hover_result: Option<Hover> =
        serde_json::from_value(hover.response_result.expect("a hover result")).unwrap();
    assert!(
        hover_result.is_some(),
        "hover on the port value returns content"
    );
    assert!(
        changed.diagnostics.is_empty(),
        "the corrected document has no diagnostics, got: {:?}",
        changed.diagnostics
    );
    assert!(
        closed.diagnostics.is_empty(),
        "closing clears the document's diagnostics"
    );
    handle.join().unwrap().unwrap();
}

#[test]
fn the_server_serves_a_yaml_document() {
    // Arrange
    let (server, client) = Connection::memory();
    let handle = std::thread::spawn(move || single(Yaml).run(&server));
    let uri = Uri::from_str("file:///server.yaml").unwrap();

    // Act, initialize.
    let _init = round_trip(&client, 1, Initialize::METHOD, InitializeParams::default());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )))
        .unwrap();

    // Act, open a YAML document with an out-of-range port.
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "yaml".to_string(),
                    version: 1,
                    text: "hostname: api\nport: 99999\n".to_string(),
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

    // Act, shut down.
    let _shutdown = round_trip(&client, 2, Shutdown::METHOD, ());
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
        "the YAML document publishes at least one diagnostic"
    );
    handle.join().unwrap().unwrap();
}

#[test]
fn the_server_advertises_and_routes_the_navigation_requests() {
    // Arrange
    let (server, client) = Connection::memory();
    let handle = std::thread::spawn(move || single(Hcl).run(&server));
    let uri = Uri::from_str("file:///server.hcl").unwrap();

    // Act, initialize and open a parsing document.
    let init = round_trip(&client, 1, Initialize::METHOD, InitializeParams::default());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "hcl".to_string(),
                    version: 1,
                    text: "hostname = \"h\"\nport = 1\nlimits {\n  mode = \"enforce\"\n}\n"
                        .to_string(),
                },
            },
        )))
        .unwrap();
    let position = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position {
            line: 0,
            character: 2,
        },
    };
    let definition = round_trip(
        &client,
        2,
        lsp_types::request::GotoDefinition::METHOD,
        lsp_types::GotoDefinitionParams {
            text_document_position_params: position.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let references = round_trip(
        &client,
        3,
        lsp_types::request::References::METHOD,
        lsp_types::ReferenceParams {
            text_document_position: position.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        },
    );
    let symbols = round_trip(
        &client,
        4,
        lsp_types::request::DocumentSymbolRequest::METHOD,
        lsp_types::DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let action = round_trip(
        &client,
        5,
        lsp_types::request::CodeActionRequest::METHOD,
        lsp_types::CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: lsp_types::Range {
                start: Position {
                    line: 0,
                    character: 12,
                },
                end: Position {
                    line: 0,
                    character: 12,
                },
            },
            context: lsp_types::CodeActionContext::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let ghost = Uri::from_str("file:///ghost.hcl").unwrap();
    let ghost_references = round_trip(
        &client,
        6,
        lsp_types::request::References::METHOD,
        lsp_types::ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: ghost },
                position: Position {
                    line: 0,
                    character: 0,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: false,
            },
        },
    );
    let document_link = round_trip(
        &client,
        7,
        lsp_types::request::DocumentLinkRequest::METHOD,
        lsp_types::DocumentLinkParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let _shutdown = round_trip(&client, 9, Shutdown::METHOD, ());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Exit::METHOD.to_string(),
            (),
        )))
        .unwrap();
    handle.join().unwrap().unwrap();

    // Assert
    let capabilities = &init.response_result.unwrap()["capabilities"];
    assert_eq!(capabilities["definitionProvider"], true);
    assert_eq!(capabilities["referencesProvider"], true);
    assert_eq!(capabilities["documentSymbolProvider"], true);
    assert_eq!(capabilities["codeActionProvider"], true);
    assert!(
        definition.response_result.is_ok(),
        "definition routes: {definition:?}"
    );
    assert!(
        references.response_result.is_ok(),
        "references routes: {references:?}"
    );

    let outline = symbols.response_result.expect("symbols route");
    assert!(!outline.is_null(), "a parsed document has an outline");
    assert!(
        action.response_result.is_ok(),
        "the code action routes: {action:?}"
    );
    assert!(
        document_link.response_result.is_ok(),
        "the document-link request routes: {document_link:?}"
    );
    assert_eq!(
        ghost_references.response_result.expect("an empty list"),
        serde_json::json!([]),
        "an unopened document answers empty"
    );
}

#[test]
fn a_document_that_does_not_parse_has_no_outline() {
    // Arrange
    let (server, client) = Connection::memory();
    let handle = std::thread::spawn(move || single(Hcl).run(&server));
    let uri = Uri::from_str("file:///broken.hcl").unwrap();

    // Act
    let _init = round_trip(&client, 1, Initialize::METHOD, InitializeParams::default());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "hcl".to_string(),
                    version: 1,
                    text: "port = = 1\n".to_string(),
                },
            },
        )))
        .unwrap();
    let symbols = round_trip(
        &client,
        2,
        lsp_types::request::DocumentSymbolRequest::METHOD,
        lsp_types::DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let _shutdown = round_trip(&client, 9, Shutdown::METHOD, ());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Exit::METHOD.to_string(),
            (),
        )))
        .unwrap();
    handle.join().unwrap().unwrap();

    // Assert
    assert_eq!(
        symbols.response_result.expect("symbols route"),
        serde_json::Value::Null,
        "the outline reads spans only a parse provides"
    );
}

#[test]
fn the_server_advertises_and_routes_the_rename_highlight_and_folding_requests() {
    // Arrange
    let (server, client) = Connection::memory();
    let handle = std::thread::spawn(move || single(Hcl).run(&server));
    let uri = Uri::from_str("file:///server.hcl").unwrap();

    // Act, initialize and open a parsing document.
    let init = round_trip(&client, 1, Initialize::METHOD, InitializeParams::default());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "hcl".to_string(),
                    version: 1,
                    text: "hostname = \"h\"\nport = 1\nlimits {\n  mode = \"enforce\"\n}\n"
                        .to_string(),
                },
            },
        )))
        .unwrap();
    let at_origin = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position {
            line: 0,
            character: 0,
        },
    };
    let folding = round_trip(
        &client,
        8,
        lsp_types::request::FoldingRangeRequest::METHOD,
        lsp_types::FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let highlight = round_trip(
        &client,
        10,
        lsp_types::request::DocumentHighlightRequest::METHOD,
        lsp_types::DocumentHighlightParams {
            text_document_position_params: at_origin.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let prepare = round_trip(
        &client,
        11,
        lsp_types::request::PrepareRenameRequest::METHOD,
        at_origin.clone(),
    );
    let rename = round_trip(
        &client,
        12,
        lsp_types::request::Rename::METHOD,
        lsp_types::RenameParams {
            text_document_position: at_origin,
            new_name: "renamed".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    let _shutdown = round_trip(&client, 9, Shutdown::METHOD, ());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Exit::METHOD.to_string(),
            (),
        )))
        .unwrap();
    handle.join().unwrap().unwrap();

    // Assert
    let capabilities = &init.response_result.unwrap()["capabilities"];
    assert_eq!(
        capabilities["foldingRangeProvider"], true,
        "the folding range provider is advertised"
    );
    assert_eq!(
        capabilities["documentHighlightProvider"], true,
        "the document highlight provider is advertised"
    );
    assert_eq!(
        capabilities["renameProvider"],
        serde_json::json!({ "prepareProvider": true }),
        "the rename provider is advertised with prepare support"
    );
    assert!(
        folding.response_result.is_ok(),
        "folding routes: {folding:?}"
    );
    assert!(
        highlight.response_result.is_ok(),
        "highlight routes: {highlight:?}"
    );
    assert!(
        prepare.response_result.is_ok(),
        "prepare rename routes: {prepare:?}"
    );
    assert!(rename.response_result.is_ok(), "rename routes: {rename:?}");
}

#[test]
fn a_refused_rename_answers_the_request_failed_error_on_the_wire() {
    // Arrange
    let (server, client) = Connection::memory();
    let handle = std::thread::spawn(move || {
        match Router::new(vec![bind::<GatewaySpec, Hcl>(Matcher::Any, Hcl)]) {
            Ok(router) => router.run(&server),
            Err(error) => panic!("one binding is never empty: {error}"),
        }
    });
    let uri = Uri::from_str("file:///gateway.hcl").unwrap();
    let text = "upstream \"api\" {\n  host = \"h\"\n  port = 1\n}\nroutes {\n  prefix = \"/a\"\n  upstream = \"api\"\n}\n";

    // Act, a rename of the label to a name holding a quote.
    let _init = round_trip(&client, 1, Initialize::METHOD, InitializeParams::default());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Initialized::METHOD.to_string(),
            InitializedParams {},
        )))
        .unwrap();
    client
        .sender
        .send(Message::Notification(Notification::new(
            DidOpenTextDocument::METHOD.to_string(),
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "hcl".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            },
        )))
        .unwrap();
    let prepare = round_trip(
        &client,
        4,
        lsp_types::request::PrepareRenameRequest::METHOD,
        TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 0,
                character: 11,
            },
        },
    );
    let at_reference = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position {
            line: 6,
            character: 15,
        },
    };
    let definition = round_trip(
        &client,
        6,
        lsp_types::request::GotoDefinition::METHOD,
        lsp_types::GotoDefinitionParams {
            text_document_position_params: at_reference.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let references = round_trip(
        &client,
        7,
        lsp_types::request::References::METHOD,
        lsp_types::ReferenceParams {
            text_document_position: at_reference.clone(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        },
    );
    let highlight = round_trip(
        &client,
        8,
        lsp_types::request::DocumentHighlightRequest::METHOD,
        lsp_types::DocumentHighlightParams {
            text_document_position_params: at_reference,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let folding = round_trip(
        &client,
        5,
        lsp_types::request::FoldingRangeRequest::METHOD,
        lsp_types::FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let rename = round_trip(
        &client,
        2,
        lsp_types::request::Rename::METHOD,
        lsp_types::RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 0,
                    character: 11,
                },
            },
            new_name: "a\"b".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        },
    );
    let _shutdown = round_trip(&client, 3, Shutdown::METHOD, ());
    client
        .sender
        .send(Message::Notification(Notification::new(
            Exit::METHOD.to_string(),
            (),
        )))
        .unwrap();
    handle.join().unwrap().unwrap();

    // Assert
    let prepared = prepare.response_result.expect("prepare routes");
    assert!(
        prepared.get("start").is_some(),
        "the label position answers a prepare range: {prepared:?}"
    );
    let folds = folding.response_result.expect("folding routes");
    assert!(
        folds.as_array().is_some_and(|folds| !folds.is_empty()),
        "the multi-line block answers at least one fold: {folds:?}"
    );
    let target = definition.response_result.expect("definition routes");
    assert!(
        target.get("uri").is_some(),
        "the reference answers its label's location: {target:?}"
    );
    let referenced = references.response_result.expect("references route");
    assert!(
        referenced.as_array().is_some_and(|list| list.len() == 2),
        "the label and the reference are listed: {referenced:?}"
    );
    let marked = highlight.response_result.expect("highlight routes");
    assert!(
        marked.as_array().is_some_and(|list| !list.is_empty()),
        "the reference answers its highlight set: {marked:?}"
    );
    let error = rename.response_result.unwrap_err();
    assert_eq!(
        error.code, -32803,
        "the request-failed code reaches the wire"
    );
    assert_eq!(
        error.message,
        "a label cannot contain a quote, a backslash, a control character, `${`, or `%{`",
        "the refusal reason reaches the client"
    );
}
