//! The reference resolution pass.
//!
//! A field marked `#[confval(references = <block>)]` holds the label of a
//! `<block>` its scope can see. Resolution searches from the reference's
//! enclosing block outward to the nearest enclosing scope whose schema declares
//! a labeled block field of that name, with the root searched last. Labels are
//! collected within that one scope instance, so two sibling instances of the
//! enclosing block may reuse a label without conflict. The whole walk reads
//! only the parsed [`Fields`](crate::format::Fields) and the
//! [`Schema`](crate::schema::Schema), so the pipeline and the language server
//! run the same check.

use std::collections::HashMap;

use crate::diagnostic::Report;
use crate::format::field::{Field, FieldKind, Fields, Scalar, ValueKind};
use crate::schema::{Constraint, Schema, SchemaType};
use crate::source::{Located, Span};

/// Checks every reference field against the labels its scope can see.
///
/// Reports a reference to a label the declaring scope does not define, at the
/// reference value's span, with the scope's defined labels in the help. The
/// walk also reports a duplicate label and an empty label within each scope
/// instance. The pass reads only `fields` and `schema`.
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
    walk_scope(fields, schema, &mut Vec::new(), &mut |event| match event {
        ScopeEvent::Scope(body, schema) => report_scope_label_issues(body, schema, report),
        ScopeEvent::Reference(site) => check_reference(&site, report),
    });
}

/// One reference occurrence the scope walk visits: the target it names, its
/// parsed value and span, and the declaring scope the outward search found.
pub struct ReferenceSite<'a> {
    /// The target block name the constraint carries.
    pub block: &'static str,
    /// The reference's parsed string value.
    pub value: String,
    /// The value's span.
    pub span: Span,
    /// The declaring scope the outward search found, its schema and instance
    /// body, or `None` when no scope on the chain declares the target.
    pub scope: Option<(&'a Schema, &'a Fields)>,
}

/// Visits every reference field below `fields`, with the declaring scope its
/// outward search resolves. The reference pass and the language server both
/// run this walk, so both resolve a reference by the same rule. Start it at
/// the root to cover a document, or at one scope instance to cover that scope.
/// In the scoped form, a site whose search stays inside the walk resolves
/// against the walk's own chain.
pub fn visit_references<'a>(
    fields: &'a Fields,
    schema: &'a Schema,
    visit: &mut dyn FnMut(ReferenceSite<'a>),
) {
    walk_scope(fields, schema, &mut Vec::new(), &mut |event| {
        if let ScopeEvent::Reference(site) = event {
            visit(site);
        }
    });
}

/// One enclosing scope on the walk: a block instance's schema and body.
struct Scope<'a> {
    schema: &'a Schema,
    body: &'a Fields,
}

/// One event of the scope walk: entering a scope instance, or a reference
/// field with its resolved declaring scope.
enum ScopeEvent<'a> {
    Scope(&'a Fields, &'a Schema),
    Reference(ReferenceSite<'a>),
}

/// Walks one scope instance and its blocks in document order, emitting a scope
/// event at each instance and a reference event at each reference field.
fn walk_scope<'a>(
    body: &'a Fields,
    schema: &'a Schema,
    chain: &mut Vec<Scope<'a>>,
    on_event: &mut dyn FnMut(ScopeEvent<'a>),
) {
    on_event(ScopeEvent::Scope(body, schema));
    chain.push(Scope { schema, body });
    for field in body.iter() {
        let Some(declared) = schema.fields.iter().find(|s| s.name == field.name) else {
            continue;
        };
        match &declared.ty {
            SchemaType::Scalar {
                constraint: Some(Constraint::References { block }),
                ..
            } => {
                if let Some((value, span)) = field_string(field) {
                    let scope = chain
                        .iter()
                        .rev()
                        .find(|scope| declares_labeled_block(scope.schema, block))
                        .map(|scope| (scope.schema, scope.body));
                    on_event(ScopeEvent::Reference(ReferenceSite {
                        block,
                        value,
                        span,
                        scope,
                    }));
                }
            }
            SchemaType::Block { schema: inner, .. } => {
                for instance in instance_bodies(field) {
                    walk_scope(instance, inner, chain, on_event);
                }
            }
            _ => {}
        }
    }
    chain.pop();
}

/// Checks one visited reference against its declaring scope's labels.
fn check_reference(site: &ReferenceSite, report: &mut Report) {
    let block = site.block;
    let Some((scope_schema, scope_body)) = site.scope else {
        // No enclosing scope declares the target. That is a schema error, so
        // the message names the target rather than the config value.
        report
            .error(format!("reference target {block} is not a labeled block"))
            .at(site.span)
            .emit();
        return;
    };
    let labels = scope_labels(scope_body, scope_schema, block);
    let defined = distinct_labels(&labels);
    if !defined.contains(&site.value.as_str()) {
        let help = if defined.is_empty() {
            format!("the file defines no {block}")
        } else {
            format!("defined {block}: {}", defined.join(", "))
        };
        let value = &site.value;
        report
            .error(format!("no {block} named {value:?}"))
            .at(site.span)
            .help(help)
            .emit();
    }
}

/// Whether `schema` declares `block` as a labeled block field of its scope.
///
/// The outward search stops at the nearest scope for which this holds. A field
/// of the same name that is not a labeled block does not stop the search, so a
/// reference field may carry its target's name.
pub fn declares_labeled_block(schema: &Schema, block: &str) -> bool {
    schema.fields.iter().any(|field| {
        field.name == block
            && matches!(&field.ty, SchemaType::Block { schema: inner, .. }
                if inner.fields.iter().any(|child| child.label))
    })
}

/// The labels the `block` field defines within one scope instance.
///
/// Each label carries its span, in document order. The list keeps every
/// instance, including a duplicate and an empty label, and the function emits
/// no diagnostics. The pipeline and the language server share it, so the editor
/// collects labels the way the reference check does.
pub fn scope_labels(scope: &Fields, schema: &Schema, block: &str) -> Vec<Located<String>> {
    let Some(label_field) = labeled_child(schema, block) else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    for instance in scope.iter().filter(|field| field.name == block) {
        for body in instance_bodies(instance) {
            if let Some((value, span)) = instance_label(body, label_field) {
                labels.push(Located::new(value, span));
            }
        }
    }
    labels
}

/// The name of the designated label field of the `block` child, or `None` when
/// `schema` does not declare `block` as a labeled block.
fn labeled_child<'a>(schema: &'a Schema, block: &str) -> Option<&'a str> {
    schema.fields.iter().find_map(|field| match &field.ty {
        SchemaType::Block { schema: inner, .. } if field.name == block => inner
            .fields
            .iter()
            .find(|child| child.label)
            .map(|child| child.name.as_str()),
        _ => None,
    })
}

/// Reports a duplicate label and an empty label within one scope instance.
///
/// The checks are scope-local: two sibling instances of the enclosing block may
/// define the same label, and a duplicate within one instance still reports.
/// Walking the schema fields rather than a map keeps the diagnostics in a
/// stable order.
fn report_scope_label_issues(body: &Fields, schema: &Schema, report: &mut Report) {
    for field in &schema.fields {
        if labeled_child(schema, &field.name).is_none() {
            continue;
        }
        let labels = scope_labels(body, schema, &field.name);
        let mut first_span: HashMap<String, Span> = HashMap::new();
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
            first_span.insert(label.value, label.span);
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
