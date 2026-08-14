//! Line and character conversion between `confval` byte offsets and LSP
//! positions.
//!
//! An LSP position is a line and a character, and the character counts code
//! units in the negotiated encoding. The default is UTF-16, so a byte offset
//! assumed to be a UTF-16 code unit misaligns a range on non-ASCII content. The
//! transport shell negotiates [`PositionEncoding`] at initialization and
//! converts through a [`LineIndex`], so the pure handlers work in byte offsets
//! throughout.
//!
//! The `line-index` crate does the conversion. This module adapts its
//! `TextSize` and `LineCol` types to the byte offsets and [`Span`] values the
//! rest of the crate uses.

use line_index::{LineCol, TextSize, WideEncoding, WideLineCol};
use lsp_types::{Position, Range};

use confval::source::Span;

/// The character encoding an LSP position counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    /// A character is a UTF-8 code unit, so it equals the byte offset.
    Utf8,
    /// A character is a UTF-16 code unit, the LSP default.
    Utf16,
}

/// Byte-offset to LSP-position conversion for one document's text.
///
/// `confval` carries a per-source line index for rendering, but that index is
/// internal and counts characters. The language server owns this one, because it
/// converts to and from a negotiated encoding rather than only for display.
#[derive(Debug)]
pub struct LineIndex {
    inner: line_index::LineIndex,
}

impl LineIndex {
    /// Builds the index for a document's text. Rebuild it when the text changes.
    pub fn new(text: &str) -> Self {
        Self {
            inner: line_index::LineIndex::new(text),
        }
    }

    /// The LSP position of a byte offset in the negotiated encoding.
    pub fn position_of(&self, text: &str, offset: usize, encoding: PositionEncoding) -> Position {
        let offset = floor_char_boundary(text, offset);
        let line_col = self.inner.line_col(TextSize::from(offset as u32));
        let character = match encoding {
            PositionEncoding::Utf8 => line_col.col,
            PositionEncoding::Utf16 => self
                .inner
                .to_wide(WideEncoding::Utf16, line_col)
                .map(|wide| wide.col)
                .unwrap_or(line_col.col),
        };
        Position {
            line: line_col.line,
            character,
        }
    }

    /// The byte offset of an LSP position in the negotiated encoding.
    ///
    /// A character past the end of its line clamps to the line's end. A line
    /// past the end of the text clamps to the end of the text.
    pub fn offset_of(&self, text: &str, position: Position, encoding: PositionEncoding) -> usize {
        let col = match encoding {
            PositionEncoding::Utf8 => position.character,
            PositionEncoding::Utf16 => self
                .inner
                .to_utf8(
                    WideEncoding::Utf16,
                    WideLineCol {
                        line: position.line,
                        col: position.character,
                    },
                )
                .map(|line_col| line_col.col)
                .unwrap_or(position.character),
        };
        let Some(range) = self.inner.line(position.line) else {
            return text.len();
        };
        let line_start = usize::from(range.start());
        let content_end = line_content_end(text, line_start, usize::from(range.end()));
        let offset = self
            .inner
            .offset(LineCol {
                line: position.line,
                col,
            })
            .map(usize::from)
            .unwrap_or(line_start);
        offset.min(content_end)
    }

    /// The LSP range of a `confval` span in the negotiated encoding.
    pub fn range_of(&self, text: &str, span: Span, encoding: PositionEncoding) -> Range {
        Range {
            start: self.position_of(text, span.start as usize, encoding),
            end: self.position_of(text, span.end as usize, encoding),
        }
    }

    /// The LSP range of a byte range in the negotiated encoding.
    pub fn range_of_bytes(
        &self,
        text: &str,
        range: (usize, usize),
        encoding: PositionEncoding,
    ) -> Range {
        Range {
            start: self.position_of(text, range.0, encoding),
            end: self.position_of(text, range.1, encoding),
        }
    }
}

/// The end of a line's content, before a trailing newline.
///
/// `line-index` ends a line's range after its `\n`, so this returns the offset
/// before that newline. The newline must fall inside the line's own range, so a
/// blank trailing line keeps its own start rather than the previous line's
/// newline.
fn line_content_end(text: &str, line_start: usize, range_end: usize) -> usize {
    if range_end > line_start && text.as_bytes().get(range_end - 1) == Some(&b'\n') {
        range_end - 1
    } else {
        range_end
    }
}

/// The largest char boundary at or before `offset`, clamped to the text length,
/// so a misaligned offset does not slice mid-code-point.
fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
