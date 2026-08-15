//! The completion handlers: attribute-name, block-type, and enum-value.
//!
//! A body position offers the fields and blocks the schema declares at the
//! cursor's path, minus the single-valued ones the parsed fields already set. A
//! repeated block stays offered, because it may recur, and a map body offers no
//! keys, because its keys are open. An attribute-value position for a keyword
//! field offers the allowed strings.
//!
//! The core is a function of the schema, the fields, and the resolved cursor
//! context. It returns items with byte-range edits, and the public handler is
//! the thin adapter that converts them through the line index and the position
//! encoding.

use std::collections::HashSet;

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, InsertTextFormat, TextEdit,
};

use confval::format::Fields;
use confval::schema::{Constraint, Schema, SchemaField, SchemaType};

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::{Absorb, CursorContext, Frontend, PositionKind};
use crate::walk::{reference_labels, repeated_block_at, resolved_level, schema_at};

/// The completion inputs: the document's schema and parse beside the resolved
/// cursor context and the buffer text.
///
/// `fields` is the parsed field tree, used to drop the fields already set. It
/// is `None` when the current buffer did not parse, in which case nothing is
/// dropped.
pub struct Cx<'a> {
    /// The root schema.
    pub schema: &'a Schema,
    /// The parsed field tree, or `None` when the buffer did not parse.
    pub fields: Option<&'a Fields>,
    /// The resolved cursor context.
    pub ctx: &'a CursorContext,
    /// The buffer text, read to apply absorption and the separator space.
    pub text: &'a str,
}

/// One completion item with its edit as a byte range, before position encoding.
#[derive(Debug, PartialEq, Eq)]
struct RawItem {
    label: String,
    kind: CompletionItemKind,
    detail: Option<String>,
    filter_text: Option<String>,
    edit: (usize, usize),
    new_text: String,
}

/// Produces the completion items for a resolved cursor.
pub fn completion<F: Frontend>(
    frontend: &F,
    cx: &Cx,
    index: &LineIndex,
    encoding: PositionEncoding,
    snippets: bool,
) -> Vec<CompletionItem> {
    raw_items(frontend, cx)
        .into_iter()
        .map(|raw| encode_item(raw, cx.text, index, encoding, snippets))
        .collect()
}

/// The completion items with byte-range edits, the pure core the table tests
/// exercise.
fn raw_items<F: Frontend>(frontend: &F, cx: &Cx) -> Vec<RawItem> {
    let Some(enclosing) = schema_at(cx.schema, &cx.ctx.path) else {
        return Vec::new();
    };
    match &cx.ctx.kind {
        PositionKind::Body => body_items(frontend, enclosing, cx),
        PositionKind::AttributeValue { field } => value_items(enclosing, field, cx),
        PositionKind::BlockLabel { .. } => Vec::new(),
    }
}

/// Attribute-name and block-type completions at a body position.
fn body_items<F: Frontend>(frontend: &F, enclosing: &Schema, cx: &Cx) -> Vec<RawItem> {
    let repeated = repeated_block_at(cx.schema, &cx.ctx.path);
    // A cursor starting a new element of a repeated block filters against
    // nothing, because the element has no fields yet. The new-element answer is
    // consulted only behind the schema's repeated-block check, so its default
    // at an unrepeated position is never read. Otherwise the resolved instance
    // body addresses the exact instance the cursor sits in, falling back to the
    // first instance only on the text recovery path.
    let set: HashSet<&str> = if repeated && cx.ctx.new_element {
        HashSet::new()
    } else {
        resolved_level(cx.ctx, cx.fields)
            .map(|level| level.iter().map(|field| field.name.as_str()).collect())
            .unwrap_or_default()
    };

    enclosing
        .fields
        .iter()
        .filter(|field| {
            matches!(field.ty, SchemaType::Block { repeated: true, .. })
                || !set.contains(field.name.as_str())
        })
        .map(|field| field_item(frontend, field, cx, repeated))
        .collect()
}

/// One completion item for a schema field.
fn field_item<F: Frontend>(frontend: &F, field: &SchemaField, cx: &Cx, repeated: bool) -> RawItem {
    let kind = if matches!(field.ty, SchemaType::Block { .. }) {
        CompletionItemKind::STRUCT
    } else {
        CompletionItemKind::FIELD
    };
    let insert = frontend.insert_text(field, &cx.ctx.path);
    // Inside a repeated block, a field opens a new sequence or array element
    // rather than a bare key.
    let new_text = if repeated && cx.ctx.new_element {
        frontend.wrap_element(insert.text)
    } else {
        insert.text
    };
    let start = absorb_left(cx.text, cx.ctx.token.0, insert.absorb, &cx.ctx.kind);
    RawItem {
        label: field.name.clone(),
        kind,
        detail: field.doc.clone(),
        filter_text: None,
        edit: (start, cx.ctx.token.1),
        new_text,
    }
}

/// The edit start after the insert's left absorption.
fn absorb_left(text: &str, start: usize, absorb: Absorb, kind: &PositionKind) -> usize {
    let bytes = text.as_bytes();
    match absorb {
        Absorb::None => start,
        Absorb::Run(byte) => {
            let mut start = start;
            while start > 0 && bytes[start - 1] == byte {
                start -= 1;
            }
            start
        }
        // The body guard keeps a one-byte absorption from eating the closing
        // quote of an adjacent value.
        Absorb::One(byte) => {
            if matches!(kind, PositionKind::Body) && start > 0 && bytes[start - 1] == byte {
                start - 1
            } else {
                start
            }
        }
    }
}

/// Enum-value and reference-value completions at an attribute-value position.
///
/// A keyword field offers its allowed strings, read from the enclosing block
/// schema. A reference field offers the labels of the block it names, collected
/// from the root schema and the parsed fields, because the target block sits
/// elsewhere in the document.
fn value_items(enclosing: &Schema, field: &str, cx: &Cx) -> Vec<RawItem> {
    let Some(target) = enclosing
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
    else {
        return Vec::new();
    };
    match &target.ty {
        SchemaType::Scalar {
            constraint: Some(Constraint::Keywords(words)),
            ..
        } => words.iter().map(|word| keyword_item(word, cx)).collect(),
        SchemaType::Scalar {
            constraint: Some(Constraint::References { block }),
            ..
        } => reference_items(block, cx),
        // A `Range` constraint bounds a number, which is typed rather than
        // chosen from a closed set, so it deliberately falls through to no
        // items.
        _ => Vec::new(),
    }
}

/// Reference-value completions: the distinct, non-empty labels the declaring
/// scope defines, offered as quoted strings. The scope is found by the same
/// outward search the reference pass runs, so the editor offers the labels the
/// pipeline accepts. Returns nothing when the buffer does not parse or no
/// enclosing scope declares the target.
fn reference_items(block: &str, cx: &Cx) -> Vec<RawItem> {
    let Some(labels) = reference_labels(cx.schema, cx.ctx, block) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    labels
        .iter()
        .filter(|label| !label.value.is_empty())
        .filter(|label| seen.insert(label.value.as_str()))
        .map(|label| keyword_item(&label.value, cx))
        .collect()
}

/// One completion item for an allowed keyword, inserted as a quoted string.
fn keyword_item(word: &str, cx: &Cx) -> RawItem {
    // A value inserted directly after the colon supplies the separating space,
    // so the completed line parses as a mapping entry rather than a plain
    // scalar that swallowed the colon.
    let after_colon = cx.ctx.token.0 > 0 && cx.text.as_bytes()[cx.ctx.token.0 - 1] == b':';
    let new_text = if after_colon {
        format!(" \"{word}\"")
    } else {
        format!("\"{word}\"")
    };
    RawItem {
        label: word.to_string(),
        kind: CompletionItemKind::ENUM_MEMBER,
        detail: None,
        // Keep the item visible when the cursor sits on a value the enum
        // members do not prefix-match, such as `loud`, by filtering against
        // that value rather than the label. Without this a client discards
        // every keyword.
        filter_text: Some(cx.ctx.token_text.clone()).filter(|current| !current.is_empty()),
        edit: cx.ctx.token,
        new_text,
    }
}

/// Converts one raw item into the LSP shape: the byte edit becomes a ranged
/// text edit under the negotiated encoding.
///
/// A block insert carries a `$0` tab stop. When the client supports snippets,
/// the edit is a snippet and the client places the cursor at the tab stop. When
/// it does not, the tab stop is removed so no literal `$0` reaches the buffer.
fn encode_item(
    raw: RawItem,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    snippets: bool,
) -> CompletionItem {
    let is_snippet = snippets && raw.new_text.contains("$0");
    let new_text = if snippets {
        raw.new_text
    } else {
        raw.new_text.replace("$0", "")
    };
    let mut item = CompletionItem {
        label: raw.label,
        kind: Some(raw.kind),
        detail: raw.detail,
        filter_text: raw.filter_text,
        ..CompletionItem::default()
    };
    if is_snippet {
        item.insert_text_format = Some(InsertTextFormat::SNIPPET);
    }
    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
        range: index.range_of_bytes(text, raw.edit, encoding),
        new_text,
    }));
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontends::{Json, Toml, Yaml};
    use confval::schema::ScalarType;

    fn scalar(name: &str) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            true,
            false,
            SchemaType::Scalar {
                leaf: ScalarType::Int,
                constraint: None,
            },
        )
    }

    fn keyword_field(name: &str) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            true,
            false,
            SchemaType::Scalar {
                leaf: ScalarType::String,
                constraint: Some(Constraint::Keywords(&["enforce", "log"])),
            },
        )
    }

    fn repeated(name: &str, fields: Vec<SchemaField>) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            true,
            false,
            SchemaType::Block {
                schema: Box::new(Schema::new(None, fields)),
                repeated: true,
            },
        )
    }

    fn context(
        path: &[&str],
        kind: PositionKind,
        token: (usize, usize),
        text: &str,
    ) -> CursorContext {
        let mut ctx = match kind {
            PositionKind::Body => {
                CursorContext::body(path.iter().map(|s| s.to_string()).collect(), token)
            }
            PositionKind::AttributeValue { field } => CursorContext::attribute_value(
                path.iter().map(|s| s.to_string()).collect(),
                field,
                token,
            ),
            PositionKind::BlockLabel { block } => CursorContext::block_label(
                path.iter().map(|s| s.to_string()).collect(),
                block,
                token,
            ),
        };
        ctx.token_text = text.get(token.0..token.1).unwrap_or_default().to_string();
        ctx
    }

    /// A (label, edit, new_text) expectation row.
    type Expected = Vec<(String, (usize, usize), String)>;

    /// A body-position table row: name, text, token, new-element, expected.
    type BodyCase = (&'static str, &'static str, (usize, usize), bool, Expected);

    /// A value-position table row: name, text, kind, token, expected.
    type ValueCase = (
        &'static str,
        &'static str,
        PositionKind,
        (usize, usize),
        Expected,
    );

    /// The (label, edit, new_text) triple of each raw item.
    fn edits(items: &[RawItem]) -> Expected {
        items
            .iter()
            .map(|item| (item.label.clone(), item.edit, item.new_text.clone()))
            .collect()
    }

    #[test]
    fn body_items_map_each_context_to_its_edits() {
        // Arrange
        // One table per body case: the same schema, with the context varying in
        // new-element state and typed prefix. The schema declares a scalar and
        // a repeated block, and the cursor path sits inside the repeated block.
        let schema = Schema::new(None, vec![repeated("rules", vec![scalar("prefix")])]);
        let cases: Vec<BodyCase> = vec![
            (
                "a fresh element line wraps the insert",
                "rules:\n  ",
                (10, 10),
                true,
                vec![("prefix".to_string(), (10, 10), "- prefix: ".to_string())],
            ),
            (
                "a field line inside an element does not wrap",
                "rules:\n  - prefix: /a\n    ",
                (26, 26),
                false,
                vec![("prefix".to_string(), (26, 26), "prefix: ".to_string())],
            ),
        ];

        for (name, text, token, new_element, expected) in cases {
            let mut ctx = context(&["rules"], PositionKind::Body, token, text);
            ctx.new_element = new_element;
            let cx = Cx {
                schema: &schema,
                fields: None,
                ctx: &ctx,
                text,
            };

            // Act
            let items = raw_items(&Yaml, &cx);

            // Assert
            assert_eq!(edits(&items), expected, "case: {name}");
        }
    }

    #[test]
    fn value_items_map_each_context_to_its_edits() {
        // Arrange
        // One table per value case: a keyword completion over a typed value,
        // over a bare colon that needs the separating space, and at a field
        // with no closed set.
        let schema = Schema::new(None, vec![keyword_field("mode"), scalar("port")]);
        let value = |field: &str| PositionKind::AttributeValue {
            field: field.to_string(),
        };
        let cases: Vec<ValueCase> = vec![
            (
                "a typed value is replaced whole",
                "mode: enf",
                value("mode"),
                (6, 9),
                vec![
                    ("enforce".to_string(), (6, 9), "\"enforce\"".to_string()),
                    ("log".to_string(), (6, 9), "\"log\"".to_string()),
                ],
            ),
            (
                "a bare colon gets the separating space",
                "mode:",
                value("mode"),
                (5, 5),
                vec![
                    ("enforce".to_string(), (5, 5), " \"enforce\"".to_string()),
                    ("log".to_string(), (5, 5), " \"log\"".to_string()),
                ],
            ),
            (
                "a range-bounded number offers nothing",
                "port: ",
                value("port"),
                (6, 6),
                vec![],
            ),
        ];

        for (name, text, kind, token, expected) in cases {
            let ctx = context(&[], kind, token, text);
            let cx = Cx {
                schema: &schema,
                fields: None,
                ctx: &ctx,
                text,
            };

            // Act
            let items = raw_items(&Yaml, &cx);

            // Assert
            assert_eq!(edits(&items), expected, "case: {name}");
        }
    }

    #[test]
    fn a_block_label_position_offers_nothing() {
        // Arrange
        let schema = Schema::new(None, vec![repeated("rules", vec![scalar("prefix")])]);
        let text = "rules \"a\" {}";
        let ctx = context(
            &[],
            PositionKind::BlockLabel {
                block: "rules".to_string(),
            },
            (7, 8),
            text,
        );
        let cx = Cx {
            schema: &schema,
            fields: None,
            ctx: &ctx,
            text,
        };

        // Act
        let items = raw_items(&Yaml, &cx);

        // Assert
        assert!(items.is_empty(), "an author names a block freely");
    }

    #[test]
    fn absorption_follows_the_insert_descriptor() {
        // Arrange
        // The TOML header absorbs the typed `[` run, and the JSON member
        // absorbs one typed quote at a body position.
        let schema = Schema::new(None, vec![repeated("rules", vec![scalar("prefix")])]);
        let toml_text = "[[ru";
        let toml_ctx = context(&[], PositionKind::Body, (2, 4), toml_text);
        let json_text = "{ \"ru";
        let json_ctx = context(&[], PositionKind::Body, (3, 5), json_text);

        // Act
        let toml_items = raw_items(
            &Toml,
            &Cx {
                schema: &schema,
                fields: None,
                ctx: &toml_ctx,
                text: toml_text,
            },
        );
        let json_items = raw_items(
            &Json,
            &Cx {
                schema: &schema,
                fields: None,
                ctx: &json_ctx,
                text: json_text,
            },
        );

        // Assert
        assert_eq!(toml_items[0].edit, (0, 4), "the `[` run is absorbed");
        assert_eq!(toml_items[0].new_text, "[[rules]]");
        assert_eq!(json_items[0].edit, (2, 5), "one quote is absorbed");
        assert_eq!(json_items[0].new_text, "\"rules\": [{ $0 }]");
    }
}
