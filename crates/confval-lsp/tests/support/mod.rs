//! The helpers the handler suites share: cursor resolution against a
//! frontend, the completion and hover plumbing for the Gateway fixture, and
//! the mesh document the scoped tests read.
#![allow(dead_code)]

use lsp_types::{CompletionTextEdit, HoverContents};

use confval::schema::ToSchema;
use confval_lsp::handlers::{ClientSupport, Cx, completion, hover};
use confval_lsp::{Frontend, Hcl, LineIndex, PositionEncoding};

use crate::fixture::GatewaySpec;

pub const ENCODING: PositionEncoding = PositionEncoding::Utf8;

/// Resolves a cursor and returns the pieces the completion and hover handlers
/// take.
pub fn at(
    text: &str,
    offset: usize,
) -> (Option<confval::format::Fields>, confval_lsp::CursorContext) {
    let tree = Hcl.parse_tree(text);
    let context = Hcl.resolve(tree.as_ref(), text, offset);
    (tree, context)
}

/// Resolves a cursor against any frontend.
pub fn at_with<F: Frontend>(
    frontend: &F,
    text: &str,
    offset: usize,
) -> (Option<confval::format::Fields>, confval_lsp::CursorContext) {
    let tree = frontend.parse_tree(text);
    let context = frontend.resolve(tree.as_ref(), text, offset);
    (tree, context)
}

/// The labels of a set of completion items.
pub fn labels(items: &[lsp_types::CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.clone()).collect()
}

/// The text a completion item inserts, read from its replace edit.
pub fn inserted(item: &lsp_types::CompletionItem) -> String {
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => edit.new_text.clone(),
        _ => item.insert_text.clone().unwrap_or_default(),
    }
}

/// The completion labels for the Gateway fixture at a cursor.
pub fn gateway_offered<F: Frontend>(frontend: &F, text: &str, offset: usize) -> Vec<String> {
    let (tree, context) = at_with(frontend, text, offset);
    let index = LineIndex::new(text);
    let schema = GatewaySpec::schema();
    let items = completion(
        frontend,
        &Cx {
            schema: &schema,
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
        ClientSupport::default(),
    );
    labels(&items)
}

/// The hover markdown for the Gateway fixture at a cursor.
pub fn gateway_hover<F: Frontend>(frontend: &F, text: &str, offset: usize) -> String {
    let (tree, context) = at_with(frontend, text, offset);
    let index = LineIndex::new(text);
    let Some(hover) = hover(
        &Cx {
            schema: &GatewaySpec::schema(),
            fields: tree.as_ref(),
            ctx: &context,
            text,
        },
        &index,
        ENCODING,
    ) else {
        panic!("a hover is produced");
    };
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("expected a markdown hover"),
    }
}

/// The mesh document the scoped editor tests share: two services, each with
/// its own labeled upstream and a route naming it.
pub const MESH_YAML: &str = "services:\n  - name: a\n    upstreams:\n      - name: ua\n        port: 1\n  - name: b\n    upstreams:\n      - name: ub\n        port: 2\n    routes:\n      - upstream: \"ub\"\n";
