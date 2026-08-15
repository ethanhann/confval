//! The hover handler.
//!
//! It resolves the field under the cursor and renders its doc comment, declared
//! type, whether it has a default, and its constraint. The IR records only that
//! a field has a default, not the rendered value, so hover states that a default
//! applies rather than printing it. It reads operator-set versus defaulted from
//! the field's presence in the parsed fields, not from a sentinel span.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use confval::format::Fields;
use confval::pipeline::label_index;
use confval::schema::{Constraint, ScalarType, Schema, SchemaField, SchemaType};

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::{CursorContext, PositionKind};
use crate::walk::{resolved_level, schema_at};

/// Produces the hover for a resolved cursor, or `None` when the cursor sits on
/// no field.
pub fn hover(
    schema: &Schema,
    fields: Option<&Fields>,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<Hover> {
    let enclosing = schema_at(schema, &ctx.path)?;
    // A cursor in a block's label names the block rather than a field.
    if let PositionKind::BlockLabel { block } = &ctx.kind {
        return Some(label_hover(block, ctx, text, index, encoding));
    }
    // A cursor on a reference value states the block it references and whether
    // the value resolves, which the generic field hover cannot show.
    if let PositionKind::AttributeValue { field } = &ctx.kind
        && let Some(target) = enclosing.fields.iter().find(|f| &f.name == field)
        && let SchemaType::Scalar {
            constraint: Some(Constraint::References { block }),
            ..
        } = &target.ty
    {
        return Some(reference_hover(
            block, fields, schema, ctx, text, index, encoding,
        ));
    }
    let name = match &ctx.kind {
        PositionKind::AttributeValue { field } => field.clone(),
        PositionKind::Body => {
            let (start, end) = ctx.token;
            text.get(start..end)?.to_string()
        }
        PositionKind::BlockLabel { .. } => return None,
    };
    let field = enclosing.fields.iter().find(|field| field.name == name)?;
    // `None` when there is no parse to read the state from, so the state is
    // unknown rather than "not set". The resolved level addresses the exact
    // instance of a repeated block, falling back to the first only on the text
    // recovery path.
    let set = resolved_level(ctx, fields).map(|level| level.has(&name));
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: render(field, set),
        }),
        range: Some(index.range_of_bytes(text, ctx.token, encoding)),
    })
}

/// Hover for a block-label position: it names the block the label belongs to.
fn label_hover(
    block: &str,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("Label of the `{block}` block."),
        }),
        range: Some(index.range_of_bytes(text, ctx.token, encoding)),
    }
}

/// Hover for a reference value: the block it names and whether the value resolves
/// against a defined label. Resolution is unknown when the buffer does not parse.
#[allow(clippy::too_many_arguments)]
fn reference_hover(
    block: &str,
    fields: Option<&Fields>,
    schema: &Schema,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Hover {
    let value = text
        .get(ctx.token.0..ctx.token.1)
        .unwrap_or_default()
        .trim_matches('"');
    let resolves = fields.map(|tree| {
        label_index(tree, schema).get(block).is_some_and(|labels| {
            labels
                .iter()
                .any(|label| !label.value.is_empty() && label.value.as_str() == value)
        })
    });
    let mut markdown = format!("References the `{block}` block.\n\n");
    match resolves {
        Some(true) => markdown.push_str("Resolves to a defined label."),
        Some(false) => markdown.push_str("Does not resolve to any defined label."),
        None => markdown.push_str("Resolution is unknown while the document does not parse."),
    }
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(index.range_of_bytes(text, ctx.token, encoding)),
    }
}

/// Renders a field's hover as Markdown. `set` is `None` when the buffer does not
/// parse, so the state line is omitted rather than guessed.
fn render(field: &SchemaField, set: Option<bool>) -> String {
    let mut out = format!("**{}**: {}\n\n", field.name, type_label(&field.ty));
    if let Some(doc) = &field.doc {
        out.push_str(doc);
        out.push_str("\n\n");
    }
    if let Some(constraint) = constraint_of(&field.ty) {
        out.push_str(&constraint_label(constraint));
        out.push_str("\n\n");
    }
    if field.has_default {
        out.push_str("Has a default.\n\n");
    }
    if let Some(set) = set {
        out.push_str(state_label(set, field.has_default));
    }
    out
}

/// The state line: operator-set, defaulted, or absent.
fn state_label(set: bool, has_default: bool) -> &'static str {
    if set {
        "Set by the configuration."
    } else if has_default {
        "Not set. Uses its default."
    } else {
        "Not set."
    }
}

/// A human label for a field's declared type.
fn type_label(ty: &SchemaType) -> &'static str {
    match ty {
        SchemaType::Scalar { leaf, .. } => scalar_label(leaf),
        SchemaType::StringList => "string list",
        SchemaType::Block { repeated: true, .. } => "block (repeatable)",
        SchemaType::Block { .. } => "block",
        SchemaType::StringMap => "map",
        _ => "value",
    }
}

/// A human label for a scalar leaf type.
fn scalar_label(leaf: &ScalarType) -> &'static str {
    match leaf {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "float",
        ScalarType::Bool => "boolean",
        ScalarType::Path => "path",
        _ => "scalar",
    }
}

/// The constraint of a scalar field, if any.
fn constraint_of(ty: &SchemaType) -> Option<&Constraint> {
    match ty {
        SchemaType::Scalar { constraint, .. } => constraint.as_ref(),
        _ => None,
    }
}

/// A human label for a constraint.
fn constraint_label(constraint: &Constraint) -> String {
    match constraint {
        Constraint::Keywords(words) => format!("One of: {}.", words.join(", ")),
        Constraint::Range {
            min,
            max,
            units,
            help,
        } => {
            let unit = units.map(|unit| format!(" {unit}")).unwrap_or_default();
            let mut label = format!("Between {min} and {max}{unit}.");
            if let Some(help) = help {
                label.push(' ');
                label.push_str(help);
            }
            label
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confval::schema::Schema;

    fn block(repeated: bool) -> SchemaType {
        SchemaType::Block {
            schema: Box::new(Schema::new(None, Vec::new())),
            repeated,
        }
    }

    #[test]
    fn type_labels_cover_every_shape() {
        // Arrange, Act, Assert
        assert_eq!(
            type_label(&SchemaType::Scalar {
                leaf: ScalarType::Int,
                constraint: None
            }),
            "integer"
        );
        assert_eq!(type_label(&SchemaType::StringList), "string list");
        assert_eq!(type_label(&SchemaType::StringMap), "map");
        assert_eq!(type_label(&block(false)), "block");
        assert_eq!(type_label(&block(true)), "block (repeatable)");
    }

    #[test]
    fn scalar_labels_cover_every_leaf() {
        // Arrange, Act, Assert
        assert_eq!(scalar_label(&ScalarType::String), "string");
        assert_eq!(scalar_label(&ScalarType::Int), "integer");
        assert_eq!(scalar_label(&ScalarType::Float), "float");
        assert_eq!(scalar_label(&ScalarType::Bool), "boolean");
        assert_eq!(scalar_label(&ScalarType::Path), "path");
    }

    #[test]
    fn constraint_labels_render_keywords_and_ranges() {
        // Arrange
        let range = Constraint::Range {
            min: "1".to_string(),
            max: "65535".to_string(),
            units: Some("ports"),
            help: Some("Pick an open port."),
        };
        let bare = Constraint::Range {
            min: "1".to_string(),
            max: "16".to_string(),
            units: None,
            help: None,
        };

        // Act, Assert
        assert_eq!(
            constraint_label(&Constraint::Keywords(&["a", "b"])),
            "One of: a, b."
        );
        assert_eq!(
            constraint_label(&range),
            "Between 1 and 65535 ports. Pick an open port."
        );
        assert_eq!(constraint_label(&bare), "Between 1 and 16.");
    }
}
