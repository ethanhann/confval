//! The completion handlers: attribute-name, block-type, and enum-value.
//!
//! A body position offers the fields and blocks the schema declares at the
//! cursor's path, minus the single-valued ones the parsed fields already set. A
//! repeated block stays offered, because it may recur, and a map body offers no
//! keys, because its keys are open. The empty map body follows from `schema_at`,
//! which returns `None` for a path that descends into an open-ended map, so no
//! special case is needed here. An attribute-value position for a keyword field
//! offers the allowed strings.
//!
//! The core is a function of the schema, the fields, and the resolved cursor
//! context. It returns items with byte-range edits, and the public handler is
//! the thin adapter that converts them through the line index and the position
//! encoding. The module splits by concern: this file produces the
//! body-position items, `values` produces the attribute-value items, and
//! `encode` converts a raw item to the LSP shape.

mod encode;
mod values;

use std::collections::HashSet;

use lsp_types::{CompletionItem, CompletionItemKind};

use confval::schema::{Schema, SchemaField, SchemaType};

use crate::encoding::{LineIndex, PositionEncoding};
#[cfg(test)]
use crate::frontend::CursorContext;
use crate::frontend::{Absorb, Frontend, PositionKind};
use crate::handlers::Cx;
use crate::walk::{repeated_block_at, resolved_level, schema_at};

use encode::encode_item;
use values::value_items;

/// The client's completion switches, read once at initialization: whether the
/// client expands snippets, and whether it honors a preselected item.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientSupport {
    /// Whether the client expands a completion snippet. Without it the
    /// markers are unwrapped, so no literal `$0` or placeholder reaches the
    /// buffer.
    pub snippets: bool,
    /// Whether the client honors a preselected item. Without it the flag is
    /// withheld, so the client's own ranking stands.
    pub preselect: bool,
}

/// One completion item with its edit as a byte range, before position encoding.
#[derive(Debug, PartialEq, Eq)]
struct RawItem {
    label: String,
    kind: CompletionItemKind,
    detail: Option<String>,
    filter_text: Option<String>,
    /// Orders the client's list by schema declaration order rather than
    /// alphabetically, so related fields stay together.
    sort_text: String,
    /// Marks the item the client should preselect: the field's default among
    /// its values. Withheld at encode time when the client lacks the support.
    preselect: bool,
    /// Whether `new_text` carries snippet markers a producer wrote. A value
    /// item's text is a literal and never sets it, so user text holding `$` or
    /// `{` is neither expanded by a snippet client nor stripped for a plain
    /// one.
    snippet: bool,
    edit: (usize, usize),
    new_text: String,
}

/// Produces the completion items for a resolved cursor.
pub fn completion<F: Frontend>(
    frontend: &F,
    cx: &Cx,
    index: &LineIndex,
    encoding: PositionEncoding,
    client: ClientSupport,
) -> Vec<CompletionItem> {
    raw_items(frontend, cx)
        .into_iter()
        .map(|raw| encode_item(raw, cx.text, index, encoding, client))
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
        PositionKind::AttributeValue { field } => value_items(frontend, enclosing, field, cx),
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
    // body addresses the exact instance the cursor is in, falling back to the
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
        .enumerate()
        .filter(|(_, field)| {
            matches!(field.ty, SchemaType::Block { repeated: true, .. })
                || !set.contains(field.name.as_str())
        })
        .map(|(order, field)| field_item(frontend, field, cx, repeated, order))
        .collect()
}

/// One completion item for a schema field.
fn field_item<F: Frontend>(
    frontend: &F,
    field: &SchemaField,
    cx: &Cx,
    repeated: bool,
    order: usize,
) -> RawItem {
    let kind = if matches!(field.ty, SchemaType::Block { .. }) {
        CompletionItemKind::STRUCT
    } else {
        CompletionItemKind::FIELD
    };
    let insert = frontend.insert_text(field, &cx.ctx.path);
    // Inside a repeated block, a field opens a new sequence or array element
    // rather than a bare key.
    let insert = if repeated && cx.ctx.new_element {
        frontend.wrap_element(insert)
    } else {
        insert
    };
    let snippet = insert.snippet;
    let start = absorb_left(cx.text, cx.ctx.token.0, insert.absorb, &cx.ctx.kind);
    let new_text = insert.text;
    RawItem {
        label: field.name.clone(),
        kind,
        detail: field.doc.clone(),
        filter_text: None,
        sort_text: sort_key(order),
        preselect: false,
        snippet,
        edit: (start, cx.ctx.token.1),
        new_text,
    }
}

/// The sort text for a declaration position: zero-padded so a client's
/// lexicographic sort matches the schema order.
fn sort_key(order: usize) -> String {
    format!("{order:04}")
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
        // The body guard keeps a one-byte absorption from taking the
        // closing quote of an adjacent value.
        Absorb::One(byte) => {
            if matches!(kind, PositionKind::Body) && start > 0 && bytes[start - 1] == byte {
                start - 1
            } else {
                start
            }
        }
    }
}

#[cfg(all(test, feature = "json", feature = "toml", feature = "yaml"))]
mod tests {
    use super::*;
    use crate::frontends::{Json, Toml, Yaml};
    use confval::schema::{Constraint, ScalarType};

    fn scalar(name: &str) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            SchemaType::Scalar {
                leaf: ScalarType::Int,
                constraint: None,
            },
        )
        .required()
    }

    fn keyword_field(name: &str) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            SchemaType::Scalar {
                leaf: ScalarType::String,
                constraint: Some(Constraint::Keywords(&["enforce", "log"])),
            },
        )
        .required()
    }

    /// A string list whose elements come from a closed set. The constraint
    /// describes one element, so completion offers the same words a scalar
    /// keyword field offers.
    fn keyword_list_field(name: &str) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            SchemaType::string_list(Some(Constraint::Keywords(&["enforce", "log"]))),
        )
        .required()
    }

    fn repeated(name: &str, fields: Vec<SchemaField>) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            SchemaType::block(Schema::new(None, fields), true),
        )
        .required()
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
    fn the_sort_key_zero_pads_the_declaration_order() {
        // Arrange, Act
        let first = sort_key(0);
        let eighth = sort_key(7);

        // Assert
        assert_eq!(first, "0000");
        assert_eq!(eighth, "0007");
        assert_ne!(first, eighth);
    }

    #[test]
    fn body_items_map_each_context_to_its_edits() {
        // Arrange
        // One table per body case: the same schema, with the context varying in
        // new-element state and typed prefix. The schema declares a scalar and
        // a repeated block, and the cursor path is inside the repeated block.
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
        let schema = Schema::new(
            None,
            vec![
                keyword_field("mode"),
                scalar("port"),
                keyword_list_field("modes"),
            ],
        );
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
            (
                "a list element offers the element keywords",
                "modes: enf",
                value("modes"),
                (7, 10),
                vec![
                    ("enforce".to_string(), (7, 10), "\"enforce\"".to_string()),
                    ("log".to_string(), (7, 10), "\"log\"".to_string()),
                ],
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
