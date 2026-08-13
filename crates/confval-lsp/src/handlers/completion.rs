//! The completion handlers: attribute-name, block-type, and enum-value.
//!
//! A body position offers the fields and blocks the schema declares at the
//! cursor's path, minus the single-valued ones the parsed fields already set. A
//! repeated block stays offered, because it may recur, and a map body offers no
//! keys, because its keys are open. An attribute-value position for a keyword
//! field offers the allowed strings.

use std::collections::HashSet;

use lsp_types::{CompletionItem, CompletionItemKind, CompletionTextEdit, TextEdit};

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
pub fn completion<F: Frontend>(
    frontend: &F,
    schema: &Schema,
    fields: Option<&Fields>,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Vec<CompletionItem> {
    let Some(enclosing) = schema_at(schema, &ctx.path) else {
        return Vec::new();
    };
    match &ctx.kind {
        PositionKind::Body => body_items(frontend, enclosing, fields, ctx, text, index, encoding),
        PositionKind::AttributeValue { field } => {
            value_items(enclosing, field, ctx, text, index, encoding)
        }
        PositionKind::BlockLabel => Vec::new(),
    }
}

/// Attribute-name and block-type completions at a body position.
fn body_items<F: Frontend>(
    frontend: &F,
    enclosing: &Schema,
    fields: Option<&Fields>,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
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
        .map(|field| field_item(frontend, field, ctx, text, index, encoding))
        .collect()
}

/// One completion item for a schema field.
fn field_item<F: Frontend>(
    frontend: &F,
    field: &SchemaField,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
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
        ..CompletionItem::default()
    };
    apply_edit(&mut item, format!("\"{word}\""), ctx, text, index, encoding);
    item
}

/// Attaches the insert text as a replace edit over the cursor's token, or as
/// plain insert text when the cursor sits on no token.
fn apply_edit(
    item: &mut CompletionItem,
    new_text: String,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) {
    match ctx.token {
        Some(range) => {
            let edit = TextEdit {
                range: index.range_of_bytes(text, range, encoding),
                new_text,
            };
            item.text_edit = Some(CompletionTextEdit::Edit(edit));
        }
        None => item.insert_text = Some(new_text),
    }
}
