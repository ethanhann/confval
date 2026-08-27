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

mod labels;

use std::collections::{HashMap, HashSet};

use crate::diagnostic::Report;
use crate::format::field::Fields;
use crate::schema::{Constraint, Schema, SchemaType};
use crate::source::Span;

use crate::format::block_bodies;
#[cfg(feature = "__internal-navigation")]
pub use labels::scope_labels;
use labels::{field_string, labeled_child, scope_label_refs};

/// Checks every reference field against the labels its scope can see.
///
/// Reports a reference to a label the declaring scope does not define, at the
/// reference value's span, with the scope's defined labels in the help. The
/// walk also reports a duplicate label and an empty label within each scope
/// instance. The pass reads only `fields` and `schema`.
///
/// The pass checks file-source string values. A value held as
/// [`Scalar::Unparsed`](crate::format::field::Scalar::Unparsed), from env-var or flag layering, is skipped, so a layered
/// reference is not checked here.
///
/// This pass reports a duplicate label, an empty label, and an undefined
/// reference. The other half of the label check is in the derived
/// `FromFields`, which reports a native label a block does not designate and a
/// child field that duplicates a native label. A caller that wants the whole
/// label story runs `from_fields` and then this pass.
pub fn check_references(fields: &Fields, schema: &Schema, report: &mut Report) {
    let mut defined = LabelCache::new();
    walk_scope(fields, schema, &mut Vec::new(), &mut |event| match event {
        ScopeEvent::Scope(body, scope_schema) => {
            report_scope_label_issues(body, scope_schema, report);
        }
        ScopeEvent::Reference(site) => check_reference(&site, schema, report, &mut defined),
    });
}

/// The labels a scope instance defines for one block, held across the
/// reference sites that resolve into that scope.
///
/// Without it, each reference collects the labels of its declaring scope
/// again. A file with many references to many instances then costs the
/// product of the two. The key is the scope instance's identity paired with
/// the target block name.
type LabelCache<'a> = HashMap<(InstanceKey, &'static str), DefinedLabels<'a>>;

/// The identity of one scope instance body.
///
/// Identity is by instance rather than by value, so two equal bodies from
/// different instances have different keys. The key is the body's address,
/// which is stable for as long as the parsed tree is borrowed.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct InstanceKey(*const Fields);

impl InstanceKey {
    /// The key of `body`.
    fn of(body: &Fields) -> Self {
        Self(std::ptr::from_ref(body))
    }
}

/// The distinct, non-empty labels of one block within one scope instance.
struct DefinedLabels<'a> {
    /// Membership, for the check itself.
    set: HashSet<&'a str>,
    /// The same values in first-occurrence order, for the help line.
    ordered: Vec<&'a str>,
}

impl<'a> DefinedLabels<'a> {
    /// Collects the block's labels within one scope instance.
    fn collect(scope: &'a Fields, schema: &Schema, block: &str) -> Self {
        let mut set = HashSet::new();
        let mut ordered = Vec::new();
        for (label, _) in scope_label_refs(scope, schema, block) {
            if !label.is_empty() && set.insert(label) {
                ordered.push(label);
            }
        }
        Self { set, ordered }
    }
}

/// One reference occurrence the scope walk visits: the target it names, its
/// parsed value and span, and the declaring scope the outward search found.
pub struct ReferenceSite<'a> {
    /// The target block name the constraint has.
    pub block: &'static str,
    /// The reference's parsed string value.
    pub value: String,
    /// The value's span.
    pub span: Span,
    /// The declaring scope the outward search found, or `None` when no scope
    /// on the chain declares the target.
    pub scope: Option<Scope<'a>>,
}

/// One scope on the reference walk: a block instance's schema and body.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    /// The scope's schema level.
    pub schema: &'a Schema,
    /// The scope's instance body.
    pub body: &'a Fields,
}

impl Scope<'_> {
    /// Whether `body` is this scope's own instance body. Identity is by
    /// instance rather than by value, so two equal bodies from different
    /// instances do not match.
    #[cfg(feature = "__internal-navigation")]
    pub fn same_instance(&self, body: &Fields) -> bool {
        InstanceKey::of(self.body) == InstanceKey::of(body)
    }
}

/// Visits every reference field below `fields`, with the declaring scope its
/// outward search resolves. The reference pass and the language server both
/// run this walk, so both resolve a reference by the same rule. Start it at
/// the root to cover a document, or at one scope instance to cover that scope.
/// In the scoped form, a site whose search stays inside the walk resolves
/// against the walk's own chain, and the root scope it reports is the same
/// instance the caller passed, which [`Scope::same_instance`] checks.
#[cfg(feature = "__internal-navigation")]
pub fn visit_references<'a>(
    fields: &'a Fields,
    schema: &'a Schema,
    mut visit: impl FnMut(ReferenceSite<'a>),
) {
    walk_scope(fields, schema, &mut Vec::new(), &mut |event| {
        if let ScopeEvent::Reference(site) = event {
            visit(site);
        }
    });
}

/// One event of the scope walk: entering a scope instance, or a reference
/// field with its resolved declaring scope.
enum ScopeEvent<'a> {
    Scope(&'a Fields, &'a Schema),
    Reference(ReferenceSite<'a>),
}

/// Walks one scope instance and its blocks in document order, emitting a scope
/// event at each instance and a reference event at each reference field. The
/// chain holds borrowed bodies and never clones, so a site's scope keeps the
/// identity of the level it was collected from.
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
                        .copied();
                    on_event(ScopeEvent::Reference(ReferenceSite {
                        block,
                        value,
                        span,
                        scope,
                    }));
                }
            }
            SchemaType::Block { schema: inner, .. } => {
                for instance in block_bodies(field) {
                    walk_scope(instance, inner, chain, on_event);
                }
            }
            _ => {}
        }
    }
    chain.pop();
}

/// Checks one visited reference against its declaring scope's labels. `root`
/// is the whole schema, read only to tell a scoping failure from a schema
/// error when no enclosing scope declares the target.
fn check_reference<'a>(
    site: &ReferenceSite<'a>,
    root: &Schema,
    report: &mut Report,
    cache: &mut LabelCache<'a>,
) {
    let block = site.block;
    let Some(Scope {
        schema: scope_schema,
        body: scope_body,
    }) = site.scope
    else {
        if declared_in_tree(root, block) {
            // The target exists, but not on this reference's chain of
            // enclosing scopes, so the cause is scoping rather than the
            // schema.
            report
                .error(format!("no {block} is in scope at this reference"))
                .at(site.span)
                .help(format!(
                    "{block} is declared in a nested scope, and a reference resolves outward through its enclosing blocks"
                ))
                .emit();
            return;
        }
        // No scope anywhere declares the target. That is a schema error, so
        // the message names the target rather than the config value.
        report
            .error(format!("reference target {block} is not a labeled block"))
            .at(site.span)
            .emit();
        return;
    };
    let defined = cache
        .entry((InstanceKey::of(scope_body), block))
        .or_insert_with(|| DefinedLabels::collect(scope_body, scope_schema, block));
    if !defined.set.contains(site.value.as_str()) {
        let help = if defined.ordered.is_empty() {
            format!("the file defines no {block}")
        } else {
            format!("defined {block}: {}", defined.ordered.join(", "))
        };
        let value = &site.value;
        report
            .error(format!("no {block} named {value:?}"))
            .at(site.span)
            .help(help)
            .emit();
    }
}

/// Whether any scope in the schema tree declares `block` as a labeled block.
fn declared_in_tree(schema: &Schema, block: &str) -> bool {
    declares_labeled_block(schema, block)
        || schema.fields.iter().any(|field| match &field.ty {
            SchemaType::Block { schema: inner, .. } => declared_in_tree(inner, block),
            _ => false,
        })
}

/// Whether `schema` declares `block` as a labeled block field of its scope.
///
/// The outward search stops at the nearest scope for which this holds. A field
/// of the same name that is not a labeled block does not stop the search, so a
/// reference field may have its target's name.
pub fn declares_labeled_block(schema: &Schema, block: &str) -> bool {
    schema.fields.iter().any(|field| {
        field.name == block
            && matches!(&field.ty, SchemaType::Block { schema: inner, .. }
                if inner.fields.iter().any(|child| child.label))
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
        let mut first_span: HashMap<&str, Span> = HashMap::new();
        for (label, span) in scope_label_refs(body, schema, &field.name) {
            if label.is_empty() {
                report
                    .error("a block label must not be empty")
                    .at(span)
                    .emit();
                continue;
            }
            if let Some(&first) = first_span.get(label) {
                report
                    .error(format!("duplicate {} label {label:?}", field.name))
                    .at(span)
                    .related(first, "first declared here")
                    .emit();
                continue;
            }
            first_span.insert(label, span);
        }
    }
}
