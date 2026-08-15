//! The hover handler.
//!
//! It resolves the field under the cursor and renders its doc comment, declared
//! type, whether it has a default, and its constraint. The IR records only that
//! a field has a default, not the rendered value, so hover states that a default
//! applies rather than printing it. It reads operator-set versus defaulted from
//! the field's presence in the parsed fields, not from a sentinel span.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use confval::format::{Field, FieldKind, Fields, Scalar, ValueKind};
use confval::schema::{Constraint, ScalarType, Schema, SchemaField, SchemaType};

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::{CursorContext, PositionKind};
use crate::walk::{reference_labels, resolved_level, schema_at};

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
        return Some(reference_hover(block, field, schema, ctx, text, index, encoding));
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

/// The compared reference value at the cursor.
enum ReferenceValue {
    /// A string value: parsed from the resolved instance body, or the raw
    /// token when the body does not hold the field yet.
    Text(String),
    /// A parsed value that is not a string. The reference pass skips it
    /// without a report, so hover states no resolution either.
    NonString,
    /// The buffer did not parse, so there is no value to compare.
    Unknown,
}

/// Hover for a reference value: the block it names and whether the value
/// resolves against a label of the declaring scope, found by the same outward
/// search the reference pass runs.
///
/// The compared value is the parsed string from the resolved instance body, so
/// a single-quoted YAML value agrees with diagnostics. The raw token stands in
/// only when the body does not hold the field, and resolution is unknown when
/// the buffer does not parse.
fn reference_hover(
    block: &str,
    field: &str,
    schema: &Schema,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Hover {
    let raw = || {
        text.get(ctx.token.0..ctx.token.1)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string()
    };
    let value = match &ctx.resolved_body {
        Some(body) => match body.get(field) {
            Some(parsed) => match field_text(parsed) {
                Some(string) => ReferenceValue::Text(string),
                None => ReferenceValue::NonString,
            },
            None => ReferenceValue::Text(raw()),
        },
        None => ReferenceValue::Unknown,
    };
    let mut markdown = format!("References the `{block}` block.");
    match value {
        ReferenceValue::Text(value) => {
            let resolves = reference_labels(schema, ctx, block).is_some_and(|labels| {
                labels
                    .iter()
                    .any(|label| !label.value.is_empty() && label.value == value)
            });
            markdown.push_str("\n\n");
            markdown.push_str(if resolves {
                "Resolves to a defined label."
            } else {
                "Does not resolve to any defined label."
            });
        }
        ReferenceValue::NonString => {}
        ReferenceValue::Unknown => {
            markdown.push_str("\n\nResolution is unknown while the document does not parse.");
        }
    }
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(index.range_of_bytes(text, ctx.token, encoding)),
    }
}

/// A parsed field's string value, or `None` when it is not a string.
fn field_text(field: &Field) -> Option<String> {
    match &field.kind {
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Scalar(Scalar::String(string)) => Some(string.clone()),
            _ => None,
        },
        _ => None,
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
        Constraint::References { block } => format!("References the `{block}` block."),
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
