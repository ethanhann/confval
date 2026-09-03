//! The attribute-value completions: enum keywords, references, booleans, and
//! rendered defaults. Each producer builds [`RawItem`](super::RawItem) values
//! against the resolved cursor, and the shared geometry helpers are in the
//! parent module.

use std::collections::HashSet;

use lsp_types::CompletionItemKind;

use confval::pipeline::is_empty_label;
use confval::schema::{Constraint, ScalarType, Schema, SchemaField, SchemaType};

use crate::frontend::{Frontend, ValueSeparator, quoted_literal};
use crate::handlers::Cx;
use crate::resolve::is_value_byte;
use crate::walk::reference_labels;

use super::{RawItem, sort_key};

/// Enum-value and reference-value completions at an attribute-value position.
///
/// A keyword field offers its allowed strings, read from the enclosing block
/// schema. A reference field offers the labels of the block it names, collected
/// from the root schema and the parsed fields, because the target block is
/// elsewhere in the document.
pub(super) fn value_items<F: Frontend + ?Sized>(
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
    // A zero-width cursor beside existing text is at the edge of an
    // element or its punctuation. The formats that separate values with
    // punctuation have no separator to write there, so an accepted item
    // would fuse with its neighbor or leave the next element without its
    // comma. Nothing is offered at such a position. The whitespace format
    // writes its separator instead, in `separated`.
    let separator = frontend.value_separator();
    if cx.ctx.token.0 == cx.ctx.token.1
        && separator != ValueSeparator::Whitespace
        && insertion_fuses(cx.text.as_bytes(), cx.ctx.token.0)
    {
        return Vec::new();
    }
    match &target.ty {
        // A list offers the same set as a scalar, once for each element the
        // operator writes. A list has no `default_text`, so no item is
        // preselected.
        SchemaType::Scalar {
            constraint: Some(Constraint::Keywords(words)),
            ..
        }
        | SchemaType::StringList {
            constraint: Some(Constraint::Keywords(words)),
            ..
        } => words
            .iter()
            .enumerate()
            .map(|(order, word)| {
                let mut item = keyword_item(word, separator, cx, order);
                // The default among the keywords is preselected rather than
                // duplicated. A default absent from the set, which the derive
                // permits, preselects nothing, because the set is
                // authoritative.
                item.preselect = target.default_text.as_deref() == Some(*word);
                item
            })
            .collect(),
        SchemaType::Scalar {
            constraint: Some(Constraint::References { block, .. }),
            ..
        } => reference_items(block, separator, cx),
        // A boolean is its own closed set. A written value offers the literal
        // it could change to, and an empty value offers both, with the
        // default preselected when the field has one.
        SchemaType::Scalar {
            leaf: ScalarType::Bool,
            constraint: None,
            ..
        } => bool_items(frontend, target, field, cx),
        // A number bounded by a `Range` and an unconstrained scalar are typed
        // rather than chosen from a closed set, so they offer only the
        // rendered default, when the field has one.
        SchemaType::Scalar { leaf, .. } => default_item(frontend, leaf, target, cx)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// The boolean literals a boolean value position offers, in the format's own
/// form. A parsed current value narrows the offer to the other literal, and
/// an unwritten value offers both, with the field's default preselected.
fn bool_items<F: Frontend + ?Sized>(
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
            new_text: separated(
                frontend.value_separator(),
                cx,
                frontend.default_literal(&ScalarType::Bool, literal),
            ),
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
fn default_item<F: Frontend + ?Sized>(
    frontend: &F,
    leaf: &ScalarType,
    target: &SchemaField,
    cx: &Cx,
) -> Option<RawItem> {
    let text = target.default_text.as_deref()?;
    let literal = frontend.default_literal(leaf, text);
    let new_text = separated(frontend.value_separator(), cx, literal);
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

/// The completed value with the separator its position needs.
///
/// A range starting directly after a mapping colon takes a leading space, so
/// the completed line parses as an entry rather than a plain scalar that
/// includes the colon. In a whitespace-separated format, a range touching a
/// name or value byte on either side takes a space on that side, so the
/// accepted item does not fuse with its neighbor.
fn separated(separator: ValueSeparator, cx: &Cx, value: String) -> String {
    let bytes = cx.text.as_bytes();
    let before = cx.ctx.token.0.checked_sub(1).and_then(|at| bytes.get(at));
    let mut result = value;
    if before == Some(&b':') {
        result.insert(0, ' ');
    }
    if separator == ValueSeparator::Whitespace {
        if before.is_some_and(|byte| is_value_byte(*byte)) {
            result.insert(0, ' ');
        }
        if bytes
            .get(cx.ctx.token.1)
            .is_some_and(|byte| is_value_byte(*byte))
        {
            result.push(' ');
        }
    }
    result
}

/// Whether a zero-width insertion at `at` would run into the text beside it.
///
/// It does not when the left neighbor is an opening bracket, a separator,
/// whitespace, or the buffer start, and the right neighbor is a closing
/// bracket, whitespace, or the buffer end. Anywhere else the inserted element
/// runs into existing text, or leaves the element after it without its comma.
fn insertion_fuses(bytes: &[u8], at: usize) -> bool {
    let left_clear = match at.checked_sub(1).and_then(|index| bytes.get(index)) {
        None => true,
        Some(byte) => byte.is_ascii_whitespace() || matches!(byte, b'[' | b'{' | b'=' | b':'),
    };
    let right_clear = match bytes.get(at) {
        None => true,
        Some(byte) => byte.is_ascii_whitespace() || matches!(byte, b']' | b'}'),
    };
    !(left_clear && right_clear)
}

/// Reference-value completions: the distinct, non-empty labels the declaring
/// scope defines, offered as quoted strings. The scope is found by the same
/// outward search the reference pass runs, so the editor offers the labels the
/// pipeline accepts. Returns nothing when the buffer does not parse or no
/// enclosing scope declares the target.
fn reference_items(block: &str, separator: ValueSeparator, cx: &Cx) -> Vec<RawItem> {
    let Some(labels) = reference_labels(cx.schema, cx.ctx, block) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    labels
        .iter()
        .filter(|label| !is_empty_label(&label.value))
        .filter(|label| seen.insert(label.value.as_str()))
        .enumerate()
        .map(|(order, label)| keyword_item(&label.value, separator, cx, order))
        .collect()
}

/// One completion item for an allowed keyword, inserted as a quoted string.
fn keyword_item(word: &str, separator: ValueSeparator, cx: &Cx, order: usize) -> RawItem {
    let new_text = separated(separator, cx, quoted_literal(word));
    RawItem {
        label: word.to_string(),
        kind: CompletionItemKind::ENUM_MEMBER,
        detail: None,
        // Keep the item visible when the cursor is on a value the enum
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::CursorContext;
    use crate::frontends::Json;

    fn ctx_at(field: &str, token: (usize, usize), text: &str) -> CursorContext {
        let mut ctx = CursorContext::attribute_value(Vec::new(), field.to_string(), token);
        ctx.token_text = text.get(token.0..token.1).unwrap_or_default().to_string();
        ctx
    }

    fn bool_field(default: Option<&str>) -> SchemaField {
        let field = SchemaField::new(
            "enabled".to_string(),
            None,
            SchemaType::scalar(ScalarType::Bool, None),
        );
        match default {
            Some(text) => field.with_default_text(text.to_string()),
            None => field,
        }
    }

    #[test]
    fn bool_items_preselects_only_the_default_literal() {
        // Arrange
        let text = "enabled: ";
        let target = bool_field(Some("true"));
        let schema = Schema::new(None, Vec::new());
        let ctx = ctx_at("enabled", (9, 9), text);
        let cx = Cx {
            schema: &schema,
            fields: None,
            ctx: &ctx,
            text,
        };

        // Act
        let items = bool_items(&Json, &target, "enabled", &cx);

        // Assert
        let flags: Vec<(String, bool)> = items
            .iter()
            .map(|item| (item.label.clone(), item.preselect))
            .collect();
        assert_eq!(
            flags,
            vec![("true".to_string(), true), ("false".to_string(), false),]
        );
    }

    #[test]
    fn default_item_keeps_the_typed_prefix_as_filter_text() {
        // Arrange
        let text = "port: 80";
        let target = SchemaField::new(
            "port".to_string(),
            None,
            SchemaType::scalar(ScalarType::Int, None),
        )
        .with_default_text("8080".to_string());
        let schema = Schema::new(None, Vec::new());
        let ctx = ctx_at("port", (6, 8), text);
        let cx = Cx {
            schema: &schema,
            fields: None,
            ctx: &ctx,
            text,
        };

        // Act
        let item = default_item(&Json, &ScalarType::Int, &target, &cx);

        // Assert
        let item = item.expect("a defaulted scalar offers its rendered default");
        assert_eq!(item.filter_text, Some("80".to_string()));
    }

    #[test]
    fn separated_returns_no_leading_space_at_the_buffer_start() {
        // Arrange
        let text = "true";
        let schema = Schema::new(None, Vec::new());
        let ctx = ctx_at("enabled", (0, 4), text);
        let cx = Cx {
            schema: &schema,
            fields: None,
            ctx: &ctx,
            text,
        };

        // Act
        let result = separated(ValueSeparator::Equals, &cx, "true".to_string());

        // Assert
        assert_eq!(result, "true");
    }
}
