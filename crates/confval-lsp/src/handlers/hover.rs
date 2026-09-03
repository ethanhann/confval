//! The hover handler.
//!
//! It resolves the field under the cursor and renders its doc comment, declared
//! type, its default, and its constraint. A scalar default prints its rendered
//! value in a format-neutral form. A defaulted shape the schema cannot render,
//! such as a list, states that a default applies. It reads operator-set versus
//! defaulted from the field's presence in the parsed fields, not from a
//! sentinel span.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use confval::format::Fields;
use confval::schema::{Constraint, ScalarType, Schema, SchemaField, SchemaType};

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::{CursorContext, PositionKind};
use crate::handlers::{Cx, string_list_element};
use crate::walk::{
    field_text, fields_at, label_matches, reference_labels, resolved_level, schema_at,
};

/// Produces the hover for a resolved cursor, or `None` when the cursor is on
/// no field.
pub fn hover(cx: &Cx, index: &LineIndex, encoding: PositionEncoding) -> Option<Hover> {
    let (schema, ctx, text) = (cx.schema, cx.ctx, cx.text);
    // A cursor inside a sequence element resolves to a body position under the
    // list's key, and the token there is the element the operator wrote rather
    // than a field name. The list is what the hover describes. This runs before
    // the descent below, which cannot enter a list and would answer nothing.
    if let Some((parent, field)) = string_list_element(cx) {
        // The context's path descends into the list, so the set state reads
        // from the enclosing level, the one that holds the list's own key.
        let level = ctx.ancestors.last().or_else(|| {
            cx.fields
                .and_then(|tree| fields_at(tree, &ctx.path[..ctx.path.len() - 1]))
        });
        return field_hover(parent, field, level, cx, index, encoding);
    }
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
            constraint: Some(Constraint::References { block, .. }),
            ..
        } = &target.ty
    {
        return Some(reference_hover(
            block, field, schema, ctx, text, index, encoding,
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
    field_hover(
        enclosing,
        &name,
        resolved_level(ctx, cx.fields),
        cx,
        index,
        encoding,
    )
}

/// The hover for one named field of `enclosing`, or `None` when the level has no
/// field by that name.
fn field_hover(
    enclosing: &Schema,
    name: &str,
    level: Option<&Fields>,
    cx: &Cx,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<Hover> {
    let field = enclosing.fields.iter().find(|field| field.name == name)?;
    // `None` means there is no parse to read the state from, so the state is
    // unknown rather than "not set".
    let set = level.map(|level| level.has(name));
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: render(field, set),
        }),
        range: Some(index.range_of_bytes(cx.text, cx.ctx.token, encoding)),
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
            let resolves = reference_labels(schema, ctx, block)
                .is_some_and(|labels| labels.iter().any(|label| label_matches(label, &value)));
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

/// Renders a field's hover as Markdown. `set` is `None` when the buffer does not
/// parse, so the state line is omitted rather than guessed.
fn render(field: &SchemaField, set: Option<bool>) -> String {
    let mut out = format!("**{}**: {}\n\n", field.name, type_label(&field.ty));
    if let Some(doc) = &field.doc {
        out.push_str(doc);
        out.push_str("\n\n");
    }
    if field.non_empty {
        out.push_str(&with_help(
            "Must not be empty.".to_string(),
            field.non_empty_help,
        ));
        out.push_str("\n\n");
    }
    if field.unique {
        out.push_str(&with_help(
            "Entries must be unique.".to_string(),
            field.unique_help,
        ));
        out.push_str("\n\n");
    }
    if let Some(constraint) = constraint_of(&field.ty) {
        out.push_str(&constraint_label(constraint));
        out.push_str("\n\n");
    }
    // A set field states its state alone, so the default lines render only
    // when the value could still fall through to the default.
    if set != Some(true) {
        if let Some(text) = &field.default_text {
            out.push_str(&format!("Defaults to {}.\n\n", neutral_value(field, text)));
        } else if field.has_default {
            out.push_str("Has a default.\n\n");
        }
    }
    if let Some(set) = set {
        out.push_str(state_label(set, field.has_default));
    }
    out
}

/// A default value in the format-neutral hover form: a string and a path
/// quoted, everything else as its text.
fn neutral_value(field: &SchemaField, text: &str) -> String {
    match &field.ty {
        SchemaType::Scalar {
            leaf: ScalarType::String | ScalarType::Path,
            ..
        } => format!("{text:?}"),
        _ => text.to_string(),
    }
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
        SchemaType::StringList { .. } => "string list",
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

/// The constraint a field records, if any.
fn constraint_of(ty: &SchemaType) -> Option<&Constraint> {
    ty.constraint()
}

/// A human label for a constraint.
fn constraint_label(constraint: &Constraint) -> String {
    match constraint {
        Constraint::Keywords(words) => format!("One of: {}.", words.join(", ")),
        Constraint::References { block, .. } => format!("References the `{block}` block."),
        Constraint::Range {
            min,
            max,
            units,
            help,
            ..
        } => {
            let unit = units.map(|unit| format!(" {unit}")).unwrap_or_default();
            with_help(format!("Between {min} and {max}{unit}."), *help)
        }
        Constraint::Length { min, max, help, .. } => {
            let label = if *min == 0 {
                format!("At most {max} characters.")
            } else {
                format!("Between {min} and {max} characters.")
            };
            with_help(label, *help)
        }
        Constraint::Format { name, .. } => format!("Format: {name}."),
        _ => String::new(),
    }
}

/// The label followed by the constraint's own help line, when it has one.
fn with_help(mut label: String, help: Option<&'static str>) -> String {
    if let Some(help) = help {
        label.push(' ');
        label.push_str(help);
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;
    use confval::schema::Schema;

    fn block(repeated: bool) -> SchemaType {
        SchemaType::block(Schema::new(None, Vec::new()), repeated)
    }

    #[test]
    fn type_labels_cover_every_shape() {
        // Arrange, Act, Assert
        assert_eq!(
            type_label(&SchemaType::scalar(ScalarType::Int, None)),
            "integer"
        );
        assert_eq!(type_label(&SchemaType::string_list(None)), "string list");
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
        let range = Constraint::range(
            "1".to_string(),
            "65535".to_string(),
            Some("ports"),
            Some("Pick an open port."),
        );
        let bare = Constraint::range("1".to_string(), "16".to_string(), None, None);

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

    #[test]
    fn constraint_labels_render_lengths() {
        // Arrange
        let helped = Constraint::length(1, 63, Some("Each DNS label is at most 63 characters."));
        let bare = Constraint::length(1, 253, None);
        let capped = Constraint::length(0, 253, None);

        // Act, Assert
        assert_eq!(
            constraint_label(&helped),
            "Between 1 and 63 characters. Each DNS label is at most 63 characters."
        );
        assert_eq!(constraint_label(&bare), "Between 1 and 253 characters.");
        assert_eq!(constraint_label(&capped), "At most 253 characters.");
    }

    #[test]
    fn constraint_labels_render_formats() {
        // Arrange
        let format = confval::constraints::format_constraint::<confval::Ipv4>();

        // Act, Assert
        assert_eq!(constraint_label(&format), "Format: IPv4 address.");
    }
}
