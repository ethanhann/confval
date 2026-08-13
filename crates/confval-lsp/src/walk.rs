//! Navigating the schema and the parsed field tree along a cursor path.
//!
//! A [`CursorContext`](crate::frontend::CursorContext) names the enclosing block
//! path. The completion and hover handlers walk the type-level [`Schema`] along
//! that path to the block that encloses the cursor, and the parsed [`Fields`]
//! along the same path to find which fields the operator has already set.

use confval::format::{FieldKind, Fields, ValueKind};
use confval::schema::{Schema, SchemaType};

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
