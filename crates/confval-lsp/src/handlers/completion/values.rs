//! The attribute-value completions: enum keywords, references, booleans, and
//! rendered defaults. Each producer builds [`RawItem`](super::RawItem) values
//! against the resolved cursor, and the shared geometry helpers live in the
//! parent module.

use std::collections::HashSet;

use lsp_types::CompletionItemKind;

use confval::schema::{Constraint, ScalarType, Schema, SchemaField, SchemaType};

use crate::frontend::{Frontend, quoted_literal};
use crate::handlers::Cx;
use crate::walk::reference_labels;

use super::{RawItem, sort_key};

/// Enum-value and reference-value completions at an attribute-value position.
///
/// A keyword field offers its allowed strings, read from the enclosing block
/// schema. A reference field offers the labels of the block it names, collected
/// from the root schema and the parsed fields, because the target block sits
/// elsewhere in the document.
pub(super) fn value_items<F: Frontend>(
    frontend: &F,
    enclosing: &Schema,
    field: &str,
    cx: &Cx,
) -> Vec<RawItem> {
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
            .enumerate()
            .map(|(order, word)| {
                let mut item = keyword_item(word, cx, order);
                // The default among the keywords is preselected rather than
                // duplicated. A default absent from the set, which the derive
                // permits, preselects nothing, because the set is
                // authoritative.
                item.preselect = target.default_text.as_deref() == Some(*word);
                item
            })
            .collect(),
        SchemaType::Scalar {
            constraint: Some(Constraint::References { block }),
            ..
        } => reference_items(block, cx),
        // A boolean is its own closed set. A written value offers the literal
        // it could change to, and an empty value offers both, with the
        // default preselected when the field carries one.
        SchemaType::Scalar {
            leaf: ScalarType::Bool,
            constraint: None,
        } => bool_items(frontend, target, field, cx),
        // A number bounded by a `Range` and an unconstrained scalar are typed
        // rather than chosen from a closed set, so they offer only the
        // rendered default, when the field carries one.
        SchemaType::Scalar { leaf, .. } => default_item(frontend, leaf, target, cx)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// The boolean literals a boolean value position offers, in the format's own
/// form. A parsed current value narrows the offer to the other literal, and
/// an unwritten value offers both, with the field's default preselected.
fn bool_items<F: Frontend>(
    frontend: &F,
    target: &SchemaField,
    field: &str,
    cx: &Cx,
) -> Vec<RawItem> {
    let current = cx
        .ctx
        .resolved_body
        .as_ref()
        .and_then(|body| body.get(field))
        .and_then(bool_value);
    let literals: &[&str] = match current {
        Some(true) => &["false"],
        Some(false) => &["true"],
        None => &["true", "false"],
    };
    literals
        .iter()
        .enumerate()
        .map(|(order, literal)| RawItem {
            label: literal.to_string(),
            kind: CompletionItemKind::ENUM_MEMBER,
            detail: None,
            filter_text: Some(cx.ctx.token_text.clone()).filter(|text| !text.is_empty()),
            sort_text: sort_key(order),
            preselect: current.is_none() && target.default_text.as_deref() == Some(*literal),
            snippet: false,
            edit: cx.ctx.token,
            new_text: separated(cx, frontend.default_literal(&ScalarType::Bool, literal)),
        })
        .collect()
}

/// A parsed field's boolean value, or `None` when it holds none.
fn bool_value(field: &confval::format::Field) -> Option<bool> {
    match &field.kind {
        confval::format::FieldKind::Value(value) => match &value.kind {
            confval::format::ValueKind::Scalar(confval::format::Scalar::Bool(flag)) => Some(*flag),
            _ => None,
        },
        _ => None,
    }
}

/// The one preselected item a defaulted scalar offers at a value position: the
/// rendered default in the format's literal form.
fn default_item<F: Frontend>(
    frontend: &F,
    leaf: &ScalarType,
    target: &SchemaField,
    cx: &Cx,
) -> Option<RawItem> {
    let text = target.default_text.as_deref()?;
    let literal = frontend.default_literal(leaf, text);
    let new_text = separated(cx, literal);
    Some(RawItem {
        label: text.to_string(),
        kind: CompletionItemKind::VALUE,
        detail: None,
        filter_text: Some(cx.ctx.token_text.clone()).filter(|current| !current.is_empty()),
        sort_text: sort_key(0),
        preselect: true,
        snippet: false,
        edit: cx.ctx.token,
        new_text,
    })
}

/// Prefixes the separating space when the replace range starts directly after
/// the colon, so the completed line parses as a mapping entry rather than a
/// plain scalar that includes the colon.
fn separated(cx: &Cx, value: String) -> String {
    if cx.ctx.token.0 > 0 && cx.text.as_bytes()[cx.ctx.token.0 - 1] == b':' {
        format!(" {value}")
    } else {
        value
    }
}

/// Reference-value completions: the distinct, non-empty labels the declaring
/// scope defines, offered as quoted strings. The scope is found by the same
/// outward search the reference pass runs, so the editor offers the labels the
/// pipeline accepts. Returns nothing when the buffer does not parse or no
/// enclosing scope declares the target.
fn reference_items(block: &str, cx: &Cx) -> Vec<RawItem> {
    let Some(labels) = reference_labels(cx.schema, cx.ctx, block) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    labels
        .iter()
        .filter(|label| !label.value.is_empty())
        .filter(|label| seen.insert(label.value.as_str()))
        .enumerate()
        .map(|(order, label)| keyword_item(&label.value, cx, order))
        .collect()
}

/// One completion item for an allowed keyword, inserted as a quoted string.
fn keyword_item(word: &str, cx: &Cx, order: usize) -> RawItem {
    let new_text = separated(cx, quoted_literal(word));
    RawItem {
        label: word.to_string(),
        kind: CompletionItemKind::ENUM_MEMBER,
        detail: None,
        // Keep the item visible when the cursor sits on a value the enum
        // members do not prefix-match, such as `loud`, by filtering against
        // that value rather than the label. Without this a client discards
        // every keyword.
        filter_text: Some(cx.ctx.token_text.clone()).filter(|current| !current.is_empty()),
        sort_text: sort_key(order),
        preselect: false,
        snippet: false,
        edit: cx.ctx.token,
        new_text,
    }
}
