//! Navigating the schema and the parsed field tree along a cursor path.
//!
//! A [`CursorContext`](crate::frontend::CursorContext) names the enclosing block
//! path. The completion and hover handlers walk the type-level [`Schema`] along
//! that path to the block that encloses the cursor, and the parsed [`Fields`]
//! along the same path to find which fields the operator has already set.

use confval::format::{FieldKind, Fields, ValueKind};
use confval::pipeline::{declares_labeled_block, scope_labels};
use confval::schema::{Schema, SchemaType};
use confval::source::Located;

use crate::frontend::CursorContext;

/// The schema of the block a cursor path encloses.
///
/// Returns `None` when a path element is not a nested block, such as a path that
/// descends into an open-ended map, which names no child schema.
pub(crate) fn schema_at<'a>(root: &'a Schema, path: &[String]) -> Option<&'a Schema> {
    let mut current = root;
    for name in path {
        let field = current.fields.iter().find(|field| &field.name == name)?;
        match &field.ty {
            SchemaType::Block { schema, .. } => current = schema,
            _ => return None,
        }
    }
    Some(current)
}

/// Whether the block at `path` is a repeated block, read from its parent's
/// field. A repeated block is a YAML sequence, a JSON array, or a TOML array of
/// tables, so a field completed inside it opens a new element.
pub(crate) fn repeated_block_at(root: &Schema, path: &[String]) -> bool {
    let Some((name, parents)) = path.split_last() else {
        return false;
    };
    let Some(parent) = schema_at(root, parents) else {
        return false;
    };
    parent.fields.iter().any(|field| {
        &field.name == name && matches!(field.ty, SchemaType::Block { repeated: true, .. })
    })
}

/// The parsed fields of the block a cursor path encloses.
///
/// Returns `None` when the tree does not carry the path, which happens for a
/// buffer that did not parse or a block the operator has not written.
pub(crate) fn fields_at<'a>(root: &'a Fields, path: &[String]) -> Option<&'a Fields> {
    let mut current = root;
    for name in path {
        let field = current.get(name)?;
        match &field.kind {
            FieldKind::Block(inner) => current = inner,
            FieldKind::Value(value) => match &value.kind {
                ValueKind::Map(inner) => current = inner,
                _ => return None,
            },
        }
    }
    Some(current)
}

/// The parsed fields of the block instance the cursor resolved into.
///
/// It is the resolved instance body when the buffer parsed, which addresses the
/// exact instance of a repeated block and reads a pending body as empty. The
/// `fields_at` fallback runs only on the text recovery path, whose context
/// carries no body because nothing parsed. The completion and hover handlers
/// read the already-set state from it.
pub(crate) fn resolved_level<'a>(
    ctx: &'a CursorContext,
    fields: Option<&'a Fields>,
) -> Option<&'a Fields> {
    ctx.resolved_body
        .as_ref()
        .or_else(|| fields.and_then(|tree| fields_at(tree, &ctx.path)))
}

/// A reference target's declaring scope: its schema and its instance body.
pub(crate) struct DeclaringScope<'a> {
    pub(crate) schema: &'a Schema,
    pub(crate) body: &'a Fields,
}

/// The declaring scope for a reference target at the cursor.
///
/// It searches outward from the cursor's scope to the nearest enclosing scope
/// whose schema declares the labeled `block`, the rule `check_references`
/// applies, reading each scope's instance body from the bodies the context
/// carries.
/// Returns `None` when no enclosing scope declares the target or when the
/// buffer did not parse, which leaves no carried bodies.
pub(crate) fn declaring_scope<'a>(
    schema: &'a Schema,
    ctx: &'a CursorContext,
    block: &str,
) -> Option<DeclaringScope<'a>> {
    let innermost = ctx.path.len();
    for depth in (0..=innermost).rev() {
        let Some(scope_schema) = schema_at(schema, &ctx.path[..depth]) else {
            continue;
        };
        if !declares_labeled_block(scope_schema, block) {
            continue;
        }
        let body = if depth == innermost {
            ctx.resolved_body.as_ref()
        } else {
            ctx.ancestors.get(depth)
        }?;
        return Some(DeclaringScope {
            schema: scope_schema,
            body,
        });
    }
    None
}

/// Whether a label is a non-empty match for a reference value.
pub(crate) fn label_matches(label: &Located<String>, value: &str) -> bool {
    !label.value.is_empty() && label.value == value
}

/// The labels a reference at the cursor resolves against: the declaring
/// scope's labels for `block`, or `None` when no scope declares it or the
/// buffer did not parse.
pub(crate) fn reference_labels(
    schema: &Schema,
    ctx: &CursorContext,
    block: &str,
) -> Option<Vec<Located<String>>> {
    let scope = declaring_scope(schema, ctx, block)?;
    Some(scope_labels(scope.body, scope.schema, block))
}
