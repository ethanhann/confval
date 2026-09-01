//! The document-highlight handler: the label and every reference that
//! resolves to it, in the open document.
//!
//! The set is the one find-references answers with the declaration included.
//! The declaration is a write occurrence and each reference is a read
//! occurrence. A client can then color the declaration apart from the
//! references.

use lsp_types::{DocumentHighlight, DocumentHighlightKind};

use confval::schema::Schema;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::CursorContext;

use super::{edit_site, label_site, span_range};

/// The highlights for the label site under the cursor, or empty on any other
/// position.
pub fn document_highlight(
    schema: &Schema,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Vec<DocumentHighlight> {
    let Some(site) = label_site(schema, ctx) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push = |span, kind| {
        if let Some(range) = span_range(span)
            && let Some(edit) = edit_site(text, range, &site.value)
        {
            out.push(DocumentHighlight {
                range: index.range_of_bytes(text, edit.range, encoding),
                kind: Some(kind),
            });
        }
    };
    if let Some(declaration) = site.declaration {
        push(declaration, DocumentHighlightKind::WRITE);
    }
    for span in site.reference_spans() {
        push(span, DocumentHighlightKind::READ);
    }
    out
}
