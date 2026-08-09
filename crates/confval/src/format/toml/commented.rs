//! The commented-out forms of the TOML write path.
//!
//! An absent optional field renders in a template as a commented-out entry,
//! and TOML has no comment node, so these builders render the entry to text
//! for [`emit_toml`](super::emit_toml) to attach as decor. Each form goes
//! through the same value mapping an active field uses, so the two cannot
//! drift.

use super::emit::{item_of_value, toml_comment};
use crate::format::EmitError;
use crate::format::emit::child_path;
use crate::format::field::{Field, FieldKind, Fields, Value, ValueKind};

/// The quoted dotted header for a level under `header`, empty at the root.
pub(super) fn child_header(header: &str, name: &str) -> String {
    let key = toml_edit::Key::new(name).to_string();
    let key = key.trim();
    if header.is_empty() {
        key.to_string()
    } else {
        format!("{header}.{key}")
    }
}

/// The commented-out form of one value field: its doc comment in the
/// spaced form, then the entry behind a spaceless `#`. The nested-list shape,
/// a non-empty sequence of maps, writes its repeated-block form so the
/// repetition stays visible.
pub(super) fn commented_value_text(
    field: &Field,
    value: &Value,
    path: &str,
    header: &str,
) -> Result<String, EmitError> {
    if let ValueKind::Seq(elements) = &value.kind
        && !elements.is_empty()
        && elements
            .iter()
            .all(|element| matches!(element.kind, ValueKind::Map(_)))
    {
        let mut out = String::from("\n");
        if let Some(doc) = &field.doc {
            out.push_str(&toml_comment(doc));
        }
        let sub_header = child_header(header, &field.name);
        for element in elements {
            let ValueKind::Map(inner) = &element.kind else {
                continue;
            };
            out.push_str(&format!("#[[{sub_header}]]\n"));
            out.push_str(&commented_level_text(inner, path, &sub_header)?);
        }
        return Ok(out);
    }
    let mut out = String::new();
    if let Some(doc) = &field.doc {
        out.push_str(&toml_comment(doc));
    }
    let (item, _) = item_of_value(value, path, header)?;
    let key = toml_edit::Key::new(&field.name).to_string();
    // A multiline string renders over several lines, and every one takes the
    // prefix, so the template reparses and uncommenting is one deletion per
    // line.
    let entry = format!("{} = {}", key.trim(), item.to_string().trim());
    for line in entry.lines() {
        out.push('#');
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// The commented-out form of one block field: its doc comment, the
/// `#[header]` line, and the level's contents behind the same prefix.
pub(super) fn commented_block_text(
    field: &Field,
    inner: &Fields,
    path: &str,
    header: &str,
) -> Result<String, EmitError> {
    let mut out = String::from("\n");
    if let Some(doc) = &field.doc {
        out.push_str(&toml_comment(doc));
    }
    let sub_header = child_header(header, &field.name);
    out.push_str(&format!("#[{sub_header}]\n"));
    out.push_str(&commented_level_text(inner, path, &sub_header)?);
    Ok(out)
}

/// The commented-out lines of a level's contents, values first and blocks
/// after, matching the active partition.
fn commented_level_text(fields: &Fields, path: &str, header: &str) -> Result<String, EmitError> {
    let mut values = String::new();
    let mut blocks = String::new();
    for field in fields.iter() {
        let child = child_path(path, &field.name);
        match &field.kind {
            FieldKind::Value(value) => {
                values.push_str(&commented_value_text(field, value, &child, header)?);
            }
            FieldKind::Block(inner) => {
                blocks.push_str(&commented_block_text(field, inner, &child, header)?);
            }
        }
    }
    values.push_str(&blocks);
    Ok(values)
}
