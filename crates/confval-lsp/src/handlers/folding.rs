//! The folding-range handler: one line range per declared block instance.
//!
//! The ranges come from the same tree the document-symbol outline builds. A
//! block folds where the outline lists it. A field the schema does not declare
//! folds nowhere. A buffer that does not parse answers empty.

use lsp_types::{FoldingRange, SymbolKind};

use confval::format::Fields;
use confval::schema::Schema;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::Recovery;

use super::symbols::{RawSymbol, raw_symbols};

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
    let brace_format = recovery.closes_with_brace();
    let mut out = Vec::new();
    collect(
        &raw_symbols(schema, fields, covers_body, text.len()),
        text,
        brace_format,
        index,
        encoding,
        &mut out,
    );
    out
}

/// Walks the symbol tree and emits one range per container that spans more
/// than one line.
fn collect(
    symbols: &[RawSymbol],
    text: &str,
    brace_format: bool,
    index: &LineIndex,
    encoding: PositionEncoding,
    out: &mut Vec<FoldingRange>,
) {
    for symbol in symbols {
        if symbol.kind == SymbolKind::STRUCT {
            let start = symbol.range.0;
            let mut end = if brace_format {
                symbol.range.1
            } else {
                symbol.content_end
            };
            end = end.min(text.len());
            while end > start && text.as_bytes()[end - 1].is_ascii_whitespace() {
                end -= 1;
            }
            if brace_format && end > start && text.as_bytes()[end - 1] == b'}' {
                end -= 1;
            }
            if end > start {
                let range = index.range_of_bytes(text, (start, end), encoding);
                if range.start.line < range.end.line {
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
        collect(&symbol.children, text, brace_format, index, encoding, out);
    }
}
