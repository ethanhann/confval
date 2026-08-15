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
use crate::source::{Located, Span};

/// Checks every reference field against the labels the document defines.
///
/// Reports a reference to a label no block defines, at the reference value's
/// span, with the defined labels in the help. Building the index also reports a
/// duplicate label and an empty label. The pass reads only `fields` and
/// `schema`.
///
/// The pass checks file-source string values. A value carried as
/// [`Scalar::Unparsed`], from env-var or flag layering, is skipped, so a layered
/// reference is not checked here.
///
/// This pass reports a duplicate label, an empty label, and an undefined
/// reference. The other half of the label check lives in the derived
/// `FromFields`, which reports a native label a block does not designate and a
/// child field that duplicates a native label. A caller that wants the whole
/// label story runs `from_fields` and then this pass.
pub fn check_references(fields: &Fields, schema: &Schema, report: &mut Report) {
    let index = label_index(fields, schema);
    report_label_issues(&index, schema, report);
    check_level(fields, schema, &index, report);
}

/// The labels each top-level, label-bearing block defines, keyed by the block's
/// field name.
///
/// Each label carries its span, in document order. The list keeps every
/// instance, including a duplicate and an empty label, and the function emits no
/// diagnostics. The pipeline and the language server share it, so the editor
/// collects labels the way the reference check does.
pub fn label_index(fields: &Fields, schema: &Schema) -> HashMap<String, Vec<Located<String>>> {
    let mut index = HashMap::new();
    for field in &schema.fields {
        let SchemaType::Block { schema: block, .. } = &field.ty else {
            continue;
        };
        let Some(label_field) = block.fields.iter().find(|child| child.label) else {
            continue;
        };
        let mut labels: Vec<Located<String>> = Vec::new();
        for instance in fields.iter().filter(|f| f.name == field.name) {
            for body in instance_bodies(instance) {
                if let Some((value, span)) = instance_label(body, &label_field.name) {
                    labels.push(Located::new(value, span));
                }
            }
        }
        index.insert(field.name.clone(), labels);
    }
    index
}

/// Reports a duplicate label and an empty label from a built index. Walking the
/// schema fields rather than the map keeps the diagnostics in a stable order.
fn report_label_issues(
    index: &HashMap<String, Vec<Located<String>>>,
    schema: &Schema,
    report: &mut Report,
) {
    for field in &schema.fields {
        let Some(labels) = index.get(&field.name) else {
            continue;
        };
        let mut first_span: HashMap<&str, Span> = HashMap::new();
        for label in labels {
            if label.value.is_empty() {
                report
                    .error("a block label must not be empty")
                    .at(label.span)
                    .emit();
                continue;
            }
            if let Some(&first) = first_span.get(label.value.as_str()) {
                report
                    .error(format!("duplicate {} label {:?}", field.name, label.value))
                    .at(label.span)
                    .related(first, "first declared here")
                    .emit();
                continue;
            }
            first_span.insert(label.value.as_str(), label.span);
        }
    }
}

/// Walks the tree, checking each reference field against the index. A reference
/// field can appear at any level, so the walk recurses through blocks.
fn check_level(
    fields: &Fields,
    schema: &Schema,
    index: &HashMap<String, Vec<Located<String>>>,
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
                match index.get(*block) {
                    // The target names no labeled top-level block. That is a
                    // schema error, so the message names the target rather than
                    // the config value.
                    None => {
                        report
                            .error(format!("reference target {block} is not a labeled block"))
                            .at(span)
                            .emit();
                    }
                    Some(labels) => {
                        let defined = distinct_labels(labels);
                        if !defined.contains(&value.as_str()) {
                            let help = if defined.is_empty() {
                                format!("the file defines no {block}")
                            } else {
                                format!("defined {block}: {}", defined.join(", "))
                            };
                            report
                                .error(format!("no {block} named {value:?}"))
                                .at(span)
                                .help(help)
                                .emit();
                        }
                    }
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

/// The distinct, non-empty label values of a block, in first-occurrence order.
fn distinct_labels(labels: &[Located<String>]) -> Vec<&str> {
    let mut distinct: Vec<&str> = Vec::new();
    for label in labels {
        if !label.value.is_empty() && !distinct.contains(&label.value.as_str()) {
            distinct.push(label.value.as_str());
        }
    }
    distinct
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
