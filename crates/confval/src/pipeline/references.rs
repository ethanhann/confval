//! The reference resolution pass.
//!
//! A field marked `#[confval(references = <block>)]` holds the label of a
//! top-level `<block>` defined in the same document. This pass builds the whole
//! label index first, so a reference resolves against a block defined later in
//! the file, then checks every reference against it. It reads only the parsed
//! [`Fields`](crate::format::Fields) and the [`Schema`](crate::schema::Schema),
//! so the pipeline and the language server run the same check.

use std::collections::HashMap;

use crate::diagnostic::Report;
use crate::format::field::{Field, FieldKind, Fields, Scalar, ValueKind};
use crate::schema::{Constraint, Schema, SchemaType};
use crate::source::Span;

/// Checks every reference field against the labels the document defines.
///
/// Reports a reference to a label no block defines, at the reference value's
/// span, with the defined labels in the help. Building the index also reports a
/// duplicate label and an empty label. The pass reads only `fields` and
/// `schema`.
pub fn check_references(fields: &Fields, schema: &Schema, report: &mut Report) {
    let index = build_index(fields, schema, report);
    check_level(fields, schema, &index, report);
}

/// The labels each top-level, label-bearing block defines, keyed by the block's
/// field name. Building it reports a duplicate and an empty label.
fn build_index(
    fields: &Fields,
    schema: &Schema,
    report: &mut Report,
) -> HashMap<String, Vec<String>> {
    let mut index = HashMap::new();
    for field in &schema.fields {
        let SchemaType::Block { schema: block, .. } = &field.ty else {
            continue;
        };
        let Some(label_field) = block.fields.iter().find(|child| child.label) else {
            continue;
        };
        let mut labels: Vec<String> = Vec::new();
        for instance in fields.iter().filter(|f| f.name == field.name) {
            for body in instance_bodies(instance) {
                let Some((value, span)) = instance_label(body, &label_field.name) else {
                    continue;
                };
                if value.is_empty() {
                    report
                        .error("a block label must not be empty")
                        .at(span)
                        .emit();
                    continue;
                }
                if labels.contains(&value) {
                    report
                        .error(format!("duplicate {} label {value:?}", field.name))
                        .at(span)
                        .emit();
                    continue;
                }
                labels.push(value);
            }
        }
        index.insert(field.name.clone(), labels);
    }
    index
}

/// Walks the tree, checking each reference field against the index. A reference
/// field can sit at any level, so the walk recurses through blocks.
fn check_level(
    fields: &Fields,
    schema: &Schema,
    index: &HashMap<String, Vec<String>>,
    report: &mut Report,
) {
    for field in fields.iter() {
        let Some(declared) = schema.fields.iter().find(|s| s.name == field.name) else {
            continue;
        };
        match &declared.ty {
            SchemaType::Scalar {
                constraint: Some(Constraint::References { block }),
                ..
            } => {
                let Some((value, span)) = field_string(field) else {
                    continue;
                };
                let defined = index
                    .get(*block)
                    .is_some_and(|labels| labels.contains(&value));
                if !defined {
                    let help = match index.get(*block) {
                        Some(labels) if !labels.is_empty() => {
                            format!("defined {block}: {}", labels.join(", "))
                        }
                        _ => format!("the file defines no {block}"),
                    };
                    report
                        .error(format!("no {block} named {value:?}"))
                        .at(span)
                        .help(help)
                        .emit();
                }
            }
            SchemaType::Block { schema: block, .. } => {
                for body in instance_bodies(field) {
                    check_level(body, block, index, report);
                }
            }
            _ => {}
        }
    }
}

/// The block bodies of one parsed field. A brace-delimited block is one body, a
/// map value is one body, and an array-of-tables value is one body per element,
/// so a repeated block reads the same in every format.
fn instance_bodies(field: &Field) -> Vec<&Fields> {
    match &field.kind {
        FieldKind::Block(body) => vec![body],
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Map(body) => vec![body],
            ValueKind::Seq(elements) => elements
                .iter()
                .filter_map(|element| match &element.kind {
                    ValueKind::Map(body) => Some(body),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        },
    }
}

/// A block instance's label: its native label slot when a frontend read one, and
/// otherwise the value of the designated label field in the body.
fn instance_label(body: &Fields, label_field: &str) -> Option<(String, Span)> {
    if let Some(label) = body.label() {
        return Some((label.value.clone(), label.span));
    }
    body.get(label_field).and_then(field_string)
}

/// A field's string scalar value and span, or `None` when it is not a string.
fn field_string(field: &Field) -> Option<(String, Span)> {
    let FieldKind::Value(value) = &field.kind else {
        return None;
    };
    match &value.kind {
        ValueKind::Scalar(Scalar::String(string)) => Some((string.clone(), value.span)),
        _ => None,
    }
}
