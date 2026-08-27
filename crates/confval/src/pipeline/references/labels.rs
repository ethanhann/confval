//! Label collection for the reference pass.
//!
//! These helpers read the labels a block defines within one scope instance.
//! The reference check, the scope label diagnostics, and the language server
//! all read labels through them, so each reader sees the same list.

use crate::format::field::{Field, FieldKind, Fields, Scalar, ValueKind, block_bodies};
use crate::schema::{Schema, SchemaType};
#[cfg(feature = "__internal-navigation")]
use crate::source::Located;
use crate::source::Span;

/// The labels the `block` field defines within one scope instance.
///
/// Each label carries its span, in document order. The list keeps every
/// instance, including a duplicate and an empty label, and the function emits
/// no diagnostics. The pipeline and the language server share it, so the editor
/// collects labels the way the reference check does.
#[cfg(feature = "__internal-navigation")]
pub fn scope_labels(scope: &Fields, schema: &Schema, block: &str) -> Vec<Located<String>> {
    scope_label_refs(scope, schema, block)
        .into_iter()
        .map(|(value, span)| Located::new(value.to_string(), span))
        .collect()
}

/// The same labels as [`scope_labels`], borrowed from the tree rather than
/// copied. Every reader inside the reference pass takes this form. The pass
/// therefore copies no label text.
pub(super) fn scope_label_refs<'a>(
    scope: &'a Fields,
    schema: &Schema,
    block: &str,
) -> Vec<(&'a str, Span)> {
    let Some(label_field) = labeled_child(schema, block) else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    for instance in scope.iter().filter(|field| field.name == block) {
        for body in block_bodies(instance) {
            if let Some((value, span)) = instance_label(body, label_field) {
                labels.push((value, span));
            }
        }
    }
    labels
}

/// The name of the designated label field of the `block` child, or `None` when
/// `schema` does not declare `block` as a labeled block.
pub(super) fn labeled_child<'a>(schema: &'a Schema, block: &str) -> Option<&'a str> {
    schema.fields.iter().find_map(|field| match &field.ty {
        SchemaType::Block { schema: inner, .. } if field.name == block => inner
            .fields
            .iter()
            .find(|child| child.label)
            .map(|child| child.name.as_str()),
        _ => None,
    })
}

/// A block instance's label: its native label slot when a frontend read one, and
/// otherwise the value of the designated label field in the body.
fn instance_label<'a>(body: &'a Fields, label_field: &str) -> Option<(&'a str, Span)> {
    if let Some(label) = body.label() {
        return Some((label.value.as_str(), label.span));
    }
    body.get(label_field).and_then(field_str)
}

/// A field's string scalar value and span, or `None` when it is not a string.
pub(super) fn field_string(field: &Field) -> Option<(String, Span)> {
    field_str(field).map(|(value, span)| (value.to_string(), span))
}

/// The same as [`field_string`], borrowed from the field.
fn field_str(field: &Field) -> Option<(&str, Span)> {
    let FieldKind::Value(value) = &field.kind else {
        return None;
    };
    match &value.kind {
        ValueKind::Scalar(Scalar::String(string)) => Some((string.as_str(), value.span)),
        _ => None,
    }
}
