//! The folding-range handler: one line range per declared field or block
//! instance that spans more than one line.
//!
//! The walk pairs each parsed field with its schema entry, the way the
//! document-symbol outline does, so a field the schema does not declare folds
//! nowhere. A block instance, a list, and a map each fold. A buffer that does
//! not parse answers empty.

use lsp_types::FoldingRange;

use confval::format::{Field, FieldKind, Fields};
use confval::schema::{Schema, SchemaType};
use confval::source::Span;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::Recovery;
use crate::resolve::{furthest_end, furthest_end_value};

use super::symbols::instances;

/// Produces the folding ranges for a parsed document.
///
/// `covers_body` is the frontend's block-span answer and `recovery` is its
/// block syntax. When a block closes with a brace the fold ends at the brace,
/// with the brace's column as the end character, so the collapsed line keeps
/// its `}` and a client that scans forward for a closing brace stops at this
/// block's own. A header or indentation format's block span runs to the next
/// sibling, and the fold ends at the block's last entry instead.
pub fn folding_ranges(
    schema: &Schema,
    fields: &Fields,
    text: &str,
    covers_body: bool,
    recovery: Recovery,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Vec<FoldingRange> {
    let walk = Walk {
        text,
        brace_format: recovery.closes_with_brace(),
        covers_body,
        index,
        encoding,
    };
    let mut out = Vec::new();
    walk.level(schema, fields, &mut out);
    out
}

/// The inputs every level of the walk shares.
struct Walk<'a> {
    text: &'a str,
    brace_format: bool,
    covers_body: bool,
    index: &'a LineIndex,
    encoding: PositionEncoding,
}

impl Walk<'_> {
    /// Folds one level: each declared field, then each block instance and
    /// its own level.
    fn level(&self, schema: &Schema, fields: &Fields, out: &mut Vec<FoldingRange>) {
        for field in fields.iter() {
            let Some(declared) = schema.fields.iter().find(|f| f.name == field.name) else {
                continue;
            };
            let field_range = self.field_extent(field);
            let SchemaType::Block { schema: inner, .. } = &declared.ty else {
                self.push(field_range, out);
                continue;
            };
            let instances = instances(field);
            // A single block instance is the field itself, and a repeated
            // header-only table starts on the line of its own first header.
            // Either would double the same first line, so the field folds
            // only when its line is its own.
            let own_line = instances.first().is_none_or(|(body, span)| {
                self.line_of(field_range.map(|r| r.0))
                    != self.line_of(self.instance_extent(body, *span).map(|r| r.0))
            });
            if own_line {
                self.push(field_range, out);
            }
            for (body, span) in instances {
                self.push(self.instance_extent(body, span), out);
                self.level(inner, body, out);
            }
        }
    }

    /// The byte extent of a whole field: from its name to the end of its
    /// content.
    fn field_extent(&self, field: &Field) -> Option<(usize, usize)> {
        let start = span_start(field.span).or_else(|| span_start(field.name_span))?;
        let end = if self.brace_format {
            span_end(field.span)
        } else {
            match &field.kind {
                FieldKind::Block(inner) => furthest_end(inner, false),
                FieldKind::Value(value) => furthest_end_value(value, false),
            }
        };
        Some((start, end as usize))
    }

    /// The byte extent of one block instance: from its span start to its
    /// span end for a brace format, or to its last entry otherwise.
    fn instance_extent(&self, body: &Fields, span: Span) -> Option<(usize, usize)> {
        let start = span_start(span)?;
        let end = if self.brace_format || (self.covers_body && !has_entries(body)) {
            span_end(span)
        } else {
            furthest_end(body, false)
        };
        Some((start, end as usize))
    }

    /// The line a byte offset is on.
    fn line_of(&self, offset: Option<usize>) -> Option<u32> {
        offset.map(|offset| {
            self.index
                .position_of(self.text, offset, self.encoding)
                .line
        })
    }

    /// Emits one range when the extent spans more than one line and no
    /// emitted range covers the same lines. A brace format's extent ends
    /// before its closing brace or bracket.
    fn push(&self, extent: Option<(usize, usize)>, out: &mut Vec<FoldingRange>) {
        let Some((start, end)) = extent else {
            return;
        };
        let bytes = self.text.as_bytes();
        let mut end = end.min(self.text.len());
        while end > start && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        if self.brace_format && end > start && matches!(bytes[end - 1], b'}' | b']') {
            end -= 1;
        }
        if end <= start {
            return;
        }
        let range = self
            .index
            .range_of_bytes(self.text, (start, end), self.encoding);
        let same_lines = |emitted: &FoldingRange| {
            emitted.start_line == range.start.line && emitted.end_line == range.end.line
        };
        if range.start.line < range.end.line && !out.iter().any(same_lines) {
            out.push(FoldingRange {
                start_line: range.start.line,
                start_character: None,
                end_line: range.end.line,
                end_character: Some(range.end.character),
                kind: None,
                collapsed_text: None,
            });
        }
    }
}

/// Whether a block body has at least one entry.
fn has_entries(body: &Fields) -> bool {
    body.iter().next().is_some()
}

/// A span's start, or `None` for the detached sentinel.
fn span_start(span: Span) -> Option<usize> {
    (!span.is_detached()).then_some(span.start as usize)
}

/// A span's end, or zero for the detached sentinel.
fn span_end(span: Span) -> u32 {
    if span.is_detached() { 0 } else { span.end }
}
