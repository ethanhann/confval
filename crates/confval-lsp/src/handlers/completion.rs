//! The completion handlers: attribute-name, block-type, and enum-value.
//!
//! A body position offers the fields and blocks the schema declares at the
//! cursor's path, minus the single-valued ones the parsed fields already set. A
//! repeated block stays offered, because it may recur, and a map body offers no
//! keys, because its keys are open. An attribute-value position for a keyword
//! field offers the allowed strings.

use std::collections::HashSet;

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, InsertTextFormat, TextEdit,
};

use confval::format::Fields;
use confval::schema::{Constraint, Schema, SchemaField, SchemaType};

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::{CursorContext, Frontend, PositionKind};
use crate::walk::{fields_at, schema_at};

/// Produces the completion items for a resolved cursor.
///
/// `fields` is the parsed field tree, used to drop the fields already set. It is
/// `None` when the current buffer did not parse, in which case nothing is
/// dropped.
#[allow(clippy::too_many_arguments)]
pub fn completion<F: Frontend>(
    frontend: &F,
    schema: &Schema,
    fields: Option<&Fields>,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    snippets: bool,
) -> Vec<CompletionItem> {
    let Some(enclosing) = schema_at(schema, &ctx.path) else {
        return Vec::new();
    };
    match &ctx.kind {
        PositionKind::Body => body_items(
            frontend, enclosing, fields, ctx, text, index, encoding, snippets,
        ),
        PositionKind::AttributeValue { field } => {
            value_items(enclosing, field, ctx, text, index, encoding)
        }
        PositionKind::BlockLabel => Vec::new(),
    }
}

/// Attribute-name and block-type completions at a body position.
#[allow(clippy::too_many_arguments)]
fn body_items<F: Frontend>(
    frontend: &F,
    enclosing: &Schema,
    fields: Option<&Fields>,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    snippets: bool,
) -> Vec<CompletionItem> {
    let set: HashSet<&str> = fields
        .and_then(|tree| fields_at(tree, &ctx.path))
        .map(|level| level.iter().map(|field| field.name.as_str()).collect())
        .unwrap_or_default();

    enclosing
        .fields
        .iter()
        .filter(|field| {
            matches!(field.ty, SchemaType::Block { repeated: true, .. })
                || !set.contains(field.name.as_str())
        })
        .map(|field| field_item(frontend, field, ctx, text, index, encoding, snippets))
        .collect()
}

/// One completion item for a schema field.
#[allow(clippy::too_many_arguments)]
fn field_item<F: Frontend>(
    frontend: &F,
    field: &SchemaField,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    snippets: bool,
) -> CompletionItem {
    let kind = if matches!(field.ty, SchemaType::Block { .. }) {
        CompletionItemKind::STRUCT
    } else {
        CompletionItemKind::FIELD
    };
    let mut item = CompletionItem {
        label: field.name.clone(),
        kind: Some(kind),
        detail: field.doc.clone(),
        ..CompletionItem::default()
    };
    apply_edit(
        &mut item,
        frontend.insert_text(field, &ctx.path),
        ctx,
        text,
        index,
        encoding,
        snippets,
    );
    item
}

/// Enum-value completions at an attribute-value position.
fn value_items(
    enclosing: &Schema,
    field: &str,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Vec<CompletionItem> {
    let Some(target) = enclosing
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
    else {
        return Vec::new();
    };
    match &target.ty {
        SchemaType::Scalar {
            constraint: Some(Constraint::Keywords(words)),
            ..
        } => words
            .iter()
            .map(|word| keyword_item(word, ctx, text, index, encoding))
            .collect(),
        _ => Vec::new(),
    }
}

/// One completion item for an allowed keyword, inserted as a quoted string.
fn keyword_item(
    word: &str,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> CompletionItem {
    let mut item = CompletionItem {
        label: word.to_string(),
        kind: Some(CompletionItemKind::ENUM_MEMBER),
        // Keep the item visible when the cursor sits on a value the enum members
        // do not prefix-match, such as `loud`, by filtering against that value
        // rather than the label. Without this a client discards every keyword.
        filter_text: text
            .get(ctx.token.0..ctx.token.1)
            .filter(|current| !current.is_empty())
            .map(str::to_string),
        ..CompletionItem::default()
    };
    // A value insert carries no tab stop, so snippet expansion does not apply.
    apply_edit(
        &mut item,
        format!("\"{word}\""),
        ctx,
        text,
        index,
        encoding,
        false,
    );
    item
}

/// Attaches the insert text as a replace edit over the cursor's token. The token
/// is a zero-width range at the cursor when there is nothing to replace, so the
/// edit inserts at the cursor rather than leaving the client to place it.
///
/// A block insert carries a `$0` tab stop. When the client supports snippets, the
/// edit is a snippet and the client places the cursor at the tab stop. When it
/// does not, the tab stop is removed so no literal `$0` reaches the buffer.
#[allow(clippy::too_many_arguments)]
fn apply_edit(
    item: &mut CompletionItem,
    new_text: String,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    snippets: bool,
) {
    let (mut start, end) = ctx.token;
    let bytes = text.as_bytes();
    // A bracketed header insert (a TOML table) replaces the bracket the operator
    // has already typed, so `[lim` becomes `[limits]` rather than `[[limits]`.
    if new_text.starts_with('[') {
        while start > 0 && bytes[start - 1] == b'[' {
            start -= 1;
        }
    } else if matches!(ctx.kind, PositionKind::Body)
        && new_text.starts_with('"')
        && start > 0
        && bytes[start - 1] == b'"'
    {
        // A JSON member insert `"key": ` at a body position replaces the opening
        // quote the operator has already typed, so `"por` becomes `"port": `
        // rather than a doubled quote. The body guard keeps it from eating the
        // closing quote of an adjacent value.
        start -= 1;
    }
    let is_snippet = snippets && new_text.contains("$0");
    let new_text = if snippets {
        new_text
    } else {
        new_text.replace("$0", "")
    };
    if is_snippet {
        item.insert_text_format = Some(InsertTextFormat::SNIPPET);
    }
    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
        range: index.range_of_bytes(text, (start, end), encoding),
        new_text,
    }));
}
