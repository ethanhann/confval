//! The commented-out spelling of the HCL write path.
//!
//! An absent optional field renders in a template as a commented-out entry,
//! and HCL has no comment node, so this builder renders the entry to text for
//! [`emit_hcl`](super::emit_hcl) to attach as decor. The entry goes through
//! the same body builder an active field uses, so the two spellings cannot
//! drift.

use super::emit::emit_body;
use crate::format::EmitError;
use crate::format::emit::comment_lines;
use crate::format::field::{Field, FieldKind, Fields, Value, ValueKind};

/// The commented-out spelling of one field: its doc comment in the spaced
/// form, then every rendered line behind a spaceless `#`. The entry renders
/// through the same body builder an active field uses, so the two spellings
/// cannot drift. The nested-list shape, a non-empty sequence of maps, spells
/// its repeated-block form, so the repetition stays visible.
pub(super) fn commented_text(field: &Field, level: usize, path: &str) -> Result<String, EmitError> {
    let indent = "  ".repeat(level);
    let mut mini = Vec::new();
    let mut block_shaped = matches!(field.kind, FieldKind::Block(_));
    match &field.kind {
        FieldKind::Value(Value {
            kind: ValueKind::Seq(elements),
            ..
        }) if !elements.is_empty()
            && elements
                .iter()
                .all(|element| matches!(element.kind, ValueKind::Map(_))) =>
        {
            block_shaped = true;
            for element in elements {
                if let ValueKind::Map(inner) = &element.kind {
                    mini.push(Field::detached_block(&field.name, inner.clone()));
                }
            }
        }
        _ => {
            let mut plain = field.clone();
            plain.commented = false;
            plain.doc = None;
            mini.push(plain);
        }
    }
    let (body, _) = emit_body(&Fields::detached(mini), level, path)?;
    let mut out = String::new();
    if block_shaped {
        out.push('\n');
    }
    if let Some(doc) = &field.doc {
        for line in comment_lines(doc) {
            out.push_str(&indent);
            if line.is_empty() {
                out.push_str("#\n");
            } else {
                out.push_str("# ");
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
    for line in body.to_string().lines() {
        // A blank line between the mini body's own blocks stays blank rather
        // than becoming a bare `#`.
        if line.is_empty() {
            out.push('\n');
            continue;
        }
        // The marker sits after the level's indent, so the entry lines up with
        // the doc comment above it. Only this level's indent moves ahead of the
        // marker, so a nested line keeps the depth it was rendered at, and
        // deleting the marker leaves every line correctly indented.
        out.push_str(&indent);
        out.push('#');
        out.push_str(line.strip_prefix(indent.as_str()).unwrap_or(line));
        out.push('\n');
    }
    Ok(out)
}
