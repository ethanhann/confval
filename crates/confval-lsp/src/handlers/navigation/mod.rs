//! Go-to-definition, find-references, rename, and document highlight over
//! the label model.
//!
//! A reference value defines nothing, so definition jumps from it to the
//! matching label in its declaring scope, found by the same outward search the
//! reference pass runs. A label is the definition, so definition answers empty
//! on it, and find-references answers with the reference values that resolve
//! to it, collected by the shared scope walk over the declaring scope
//! instance. [`rename`] and [`document_highlight`] read the same site.
//! Rename edits it and highlight marks it. Every handler answers empty on
//! a buffer that does not parse, because navigation reads spans only a
//! parse provides.

mod edit;
mod highlight;
mod rename;

pub(super) use edit::span_range;
use edit::{EditSite, Quote, edit_site};
pub use highlight::document_highlight;
pub use rename::{prepare_rename, rename};

use lsp_types::{Location, Uri};

use confval::pipeline::{Scope, is_empty_label, scope_labels, visit_references};
use confval::schema::{Constraint, Schema, SchemaType};
use confval::source::Span;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::{CursorContext, PositionKind};
use crate::handlers::Cx;
use crate::walk::{declaring_scope, field_text, label_matches, schema_at};

/// The label site a cursor resolves to: the declaring scope, the block field
/// the labels belong to, the label value under the cursor, and where that
/// label is declared.
pub(super) struct LabelSite<'a> {
    scope: Scope<'a>,
    block: String,
    pub(super) value: String,
    pub(super) declaration: Option<Span>,
    /// Whether the cursor is on a reference value rather than on the label
    /// itself. Definition answers only here, because a label is its own
    /// definition.
    is_reference: bool,
}

/// The definition of the reference under the cursor: the span of the matching
/// label in the declaring scope, or `None` on a label, an unresolved value, or
/// any other position.
pub fn definition(
    cx: &Cx,
    uri: &Uri,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<Location> {
    let site = label_site(cx.schema, cx.ctx)?;
    if !site.is_reference {
        return None;
    }
    let span = site.declaration?;
    Some(location(span, uri, cx.text, index, encoding))
}

/// Every reference value that resolves to the label under the cursor, with the
/// label's own span joining the list when `include_declaration` is set.
pub fn references(
    cx: &Cx,
    include_declaration: bool,
    uri: &Uri,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Vec<Location> {
    let Some(site) = label_site(cx.schema, cx.ctx) else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    if include_declaration && let Some(declaration) = site.declaration {
        spans.push(declaration);
    }
    spans.extend(site.reference_spans());
    let mut locations = Vec::with_capacity(spans.len());
    for span in spans {
        locations.push(location(span, uri, cx.text, index, encoding));
    }
    locations
}

impl LabelSite<'_> {
    /// The span of every reference value that resolves to this site's label,
    /// in walk order.
    ///
    /// The walk covers the declaring scope instance's subtree. A site whose
    /// own outward search resolves to a nearer scope keeps that scope instead.
    /// The scope-instance comparison then drops the shadowed references.
    pub(super) fn reference_spans(&self) -> Vec<Span> {
        let scope_body = self.scope.body;
        let mut spans = Vec::new();
        visit_references(scope_body, self.scope.schema, |candidate| {
            let Some(candidate_scope) = candidate.scope else {
                return;
            };
            if candidate.block == self.block
                && candidate_scope.same_instance(scope_body)
                && candidate.value == self.value
            {
                spans.push(candidate.span);
            }
        });
        spans
    }

    /// Whether the declaring scope declares this site's label value more
    /// than once. Such a scope is not renameable, because the edit would be
    /// ambiguous. The validator already reports the duplicate.
    pub(super) fn has_duplicate_label(&self) -> bool {
        scope_labels(self.scope.body, self.scope.schema, &self.block)
            .iter()
            .filter(|label| label.value == self.value)
            .count()
            > 1
    }

    /// Whether the declaring scope declares `name` on another label. A rename
    /// onto such a name would create a duplicate.
    pub(super) fn declares_other_label(&self, name: &str) -> bool {
        self.value != name
            && scope_labels(self.scope.body, self.scope.schema, &self.block)
                .iter()
                .any(|label| label.value == name)
    }
}

/// Classifies the cursor against the label positions: a reference value, a
/// native block label, or the designated label field's value. Any other
/// position, including the label field's name, is no site.
pub(super) fn label_site<'a>(schema: &'a Schema, ctx: &'a CursorContext) -> Option<LabelSite<'a>> {
    match &ctx.kind {
        PositionKind::AttributeValue { field } => {
            let enclosing = schema_at(schema, &ctx.path)?;
            let target = enclosing.fields.iter().find(|f| &f.name == field)?;
            if let SchemaType::Scalar {
                constraint: Some(Constraint::References { block, .. }),
                ..
            } = &target.ty
            {
                return reference_site(schema, ctx, field, block);
            }
            if target.label {
                return label_field_site(schema, ctx, field);
            }
            None
        }
        PositionKind::BlockLabel { block } => native_label_site(schema, ctx, block),
        PositionKind::Body => None,
    }
}

/// The site for a cursor on a reference value. The declaration is the first
/// matching label in document order, a deterministic pick for a buffer whose
/// duplicate labels diagnostics already flag.
fn reference_site<'a>(
    schema: &'a Schema,
    ctx: &'a CursorContext,
    field: &str,
    block: &'static str,
) -> Option<LabelSite<'a>> {
    let body = ctx.resolved_body.as_ref()?;
    let value = match body.get(field) {
        Some(parsed) => field_text(parsed)?,
        None => ctx.token_text.trim_matches('"').to_string(),
    };
    let scope = declaring_scope(schema, ctx, block)?;
    let declaration = scope_labels(scope.body, scope.schema, block)
        .into_iter()
        .find(|label| label_matches(label, &value))
        .map(|label| label.span);
    Some(LabelSite {
        scope,
        block: block.to_string(),
        value,
        declaration,
        is_reference: true,
    })
}

/// The site for a cursor on the designated label field's value, the label form
/// of TOML, JSON, and YAML. The declaring scope is the parent of the labeled
/// block instance, kept as the last ancestor.
fn label_field_site<'a>(
    schema: &'a Schema,
    ctx: &'a CursorContext,
    field: &str,
) -> Option<LabelSite<'a>> {
    let block = ctx.path.last()?.clone();
    let parent = &ctx.path[..ctx.path.len() - 1];
    let scope_schema = schema_at(schema, parent)?;
    let scope_body = ctx.ancestors.last()?;
    let instance = ctx.resolved_body.as_ref()?;
    let label = instance.get(field)?;
    let value = field_text(label)?;
    // The pipeline rejects an empty label, so it defines nothing to navigate.
    if is_empty_label(&value) {
        return None;
    }
    Some(LabelSite {
        scope: Scope {
            schema: scope_schema,
            body: scope_body,
        },
        block,
        value,
        declaration: value_span(label),
        is_reference: false,
    })
}

/// The site for a cursor in a native HCL or KDL block label, whose context
/// path and resolved body already name the declaring scope.
fn native_label_site<'a>(
    schema: &'a Schema,
    ctx: &'a CursorContext,
    block: &str,
) -> Option<LabelSite<'a>> {
    let scope_schema = schema_at(schema, &ctx.path)?;
    let scope_body = ctx.resolved_body.as_ref()?;
    let offset = ctx.token.0 as u32;
    let label = scope_labels(scope_body, scope_schema, block)
        .into_iter()
        .find(|label| {
            !label.span.is_detached() && label.span.start <= offset && offset <= label.span.end
        })?;
    if is_empty_label(&label.value) {
        return None;
    }
    Some(LabelSite {
        scope: Scope {
            schema: scope_schema,
            body: scope_body,
        },
        block: block.to_string(),
        value: label.value,
        declaration: Some(label.span),
        is_reference: false,
    })
}

/// The span of a field's parsed value, or `None` when it has none.
fn value_span(field: &confval::format::Field) -> Option<Span> {
    match &field.kind {
        confval::format::FieldKind::Value(value) if !value.span.is_detached() => Some(value.span),
        _ => None,
    }
}

/// Converts a span into a protocol location under the negotiated encoding.
fn location(
    span: Span,
    uri: &Uri,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Location {
    Location {
        uri: uri.clone(),
        range: index.range_of_bytes(text, (span.start as usize, span.end as usize), encoding),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confval::format::{Field, Scalar, Value, ValueKind};

    #[test]
    fn value_span_is_none_for_a_detached_value() {
        // Arrange
        let value = Value::detached(ValueKind::Scalar(Scalar::String("h".to_string())));
        let field = Field::detached_value("host", value);

        // Act
        let span = value_span(&field);

        // Assert
        assert!(
            span.is_none(),
            "a detached value has no location to navigate to"
        );
    }

    #[cfg(feature = "hcl")]
    #[test]
    fn value_span_answers_the_value_span_for_a_located_value() {
        // Arrange
        use crate::frontend::Frontend;
        let text = "host = \"h\"\n";
        let (fields, _report) = crate::frontends::Hcl.parse_buffer(text);
        let fields = fields.expect("the buffer parses");
        let field = fields.get("host").expect("the host field");
        let confval::format::FieldKind::Value(value) = &field.kind else {
            panic!("an attribute value");
        };
        let expected = value.span;

        // Act
        let span = value_span(field);

        // Assert
        assert_eq!(span, Some(expected), "a located value returns its own span");
    }
}
