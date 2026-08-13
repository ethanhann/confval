//! Line and character conversion between `confval` byte offsets and LSP
//! positions.
//!
//! An LSP position is a line and a character, and the character counts code
//! units in the negotiated encoding. The default is UTF-16, so a byte offset
//! assumed to be a UTF-16 code unit misaligns a range on non-ASCII content. The
//! transport shell negotiates [`PositionEncoding`] at initialization and
//! converts through a [`LineIndex`], so the pure handlers work in byte offsets
//! throughout.

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

impl PositionEncoding {
    /// The number of code units `ch` occupies in this encoding.
    fn code_units(self, ch: char) -> usize {
        match self {
            PositionEncoding::Utf8 => ch.len_utf8(),
            PositionEncoding::Utf16 => ch.len_utf16(),
        }
    }
}

/// Byte offsets of each line start, for offset-to-position conversion.
///
/// `confval` carries a per-source line index for rendering, but that index is
/// internal and counts characters. The language server owns this one, because it
/// converts to and from a negotiated encoding rather than only for display.
#[derive(Debug)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Builds the index for a document's text.
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }
        Self { line_starts }
    }

    /// The zero-based line containing `offset`.
    fn line_at(&self, offset: usize) -> usize {
        self.line_starts
            .binary_search(&offset)
            .unwrap_or_else(|insertion| insertion.saturating_sub(1))
    }

    /// The LSP position of a byte offset in the negotiated encoding.
    pub fn position_of(&self, text: &str, offset: usize, encoding: PositionEncoding) -> Position {
        let offset = floor_char_boundary(text, offset);
        let line = self.line_at(offset);
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        let segment = text.get(line_start..offset).unwrap_or("");
        let character: usize = segment.chars().map(|ch| encoding.code_units(ch)).sum();
        Position {
            line: line as u32,
            character: character as u32,
        }
    }

    /// The byte offset of an LSP position in the negotiated encoding.
    pub fn offset_of(&self, text: &str, position: Position, encoding: PositionEncoding) -> usize {
        let line = position.line as usize;
        let line_start = self.line_starts.get(line).copied().unwrap_or(text.len());
        let target = position.character as usize;
        let mut units = 0usize;
        let mut offset = line_start;
        for ch in text.get(line_start..).unwrap_or("").chars() {
            if ch == '\n' || units >= target {
                break;
            }
            units += encoding.code_units(ch);
            offset += ch.len_utf8();
        }
        offset
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

/// The largest char boundary at or before `offset`, clamped to the text length,
/// so a misaligned offset does not slice mid-code-point.
fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
