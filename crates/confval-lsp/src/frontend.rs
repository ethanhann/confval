//! The format seam: the [`Frontend`] trait and the [`CursorContext`] its
//! resolution produces.
//!
//! A frontend delegates parsing to `confval`, retains the last neutral field
//! tree that parsed, resolves a byte offset to a cursor context, and renders a
//! field's insert text in its format. Position resolution is the only new work.
//! The block-structured formats resolve through one shared walk over the neutral
//! field tree, so each frontend's [`resolve`](Frontend::resolve) is the default.

use confval::diagnostic::Report;
use confval::format::Fields;
use confval::schema::SchemaField;
use confval::source::{SourceId, SourceMap};

use crate::resolve::resolve_in_tree;

/// The kind of position a cursor sits in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionKind {
    /// A body position, where an attribute name or a block type is legal.
    Body,
    /// An attribute-value position for the named field.
    AttributeValue {
        /// The name of the field whose value the cursor sits in.
        field: String,
    },
    /// A block-label position for the enclosing block. Resolution does not yet
    /// produce this variant. It is reserved for the label-completion follow-on,
    /// which adds the producer and the handler behavior.
    BlockLabel,
}

/// The resolved query result the handlers read.
///
/// It names the schema path from the root to the block that encloses the cursor,
/// the kind of position the cursor sits in, and the byte range of the identifier
/// or value under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorContext {
    /// The schema path from the root to the block that encloses the cursor, each
    /// element the field name of a block the cursor sits inside.
    pub path: Vec<String>,
    /// The kind of position the cursor sits in.
    pub kind: PositionKind,
    /// The byte range in the current text that a completion replaces: the
    /// identifier or value under the cursor, or a zero-width range at the cursor
    /// when it sits on no token. It is scanned from the current text, not the
    /// retained tree, so it stays valid and on the cursor's line even when the
    /// buffer does not parse.
    pub token: (usize, usize),
}

impl CursorContext {
    /// A body position at `path` with the given replace token.
    pub(crate) fn body(path: Vec<String>, token: (usize, usize)) -> Self {
        Self {
            path,
            kind: PositionKind::Body,
            token,
        }
    }

    /// An attribute-value position for `field` at `path`.
    pub(crate) fn attribute_value(path: Vec<String>, field: String, token: (usize, usize)) -> Self {
        Self {
            path,
            kind: PositionKind::AttributeValue { field },
            token,
        }
    }
}

/// The one format-dependent seam.
///
/// A frontend binds one format's parse function and insert spelling. Parsing and
/// resolution reuse `confval`'s machinery, so the block-structured formats share
/// the default [`parse_tree`](Frontend::parse_tree) and [`resolve`](Frontend::resolve).
pub trait Frontend {
    /// Parses the buffer into the neutral field model, appending to `report`.
    /// Delegates to the format's existing `confval` parse function, so
    /// diagnostics reuse the real pipeline rather than an approximation.
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields>;

    /// Parses `text` into the retained tree, the neutral [`Fields`]. The document
    /// store keeps the last `Some` result, so a later invalid buffer still has a
    /// tree to resolve against. A throwaway [`SourceMap`] holds the text, because
    /// resolution reads only byte offsets, which are the same in any map.
    fn parse_tree(&self, text: &str) -> Option<Fields> {
        let mut sources = SourceMap::new();
        let id = sources.add("<buffer>", text);
        let mut report = Report::new();
        self.parse(&sources, id, &mut report)
    }

    /// Resolves a byte offset to the cursor context. It reads the retained tree
    /// and scans the raw text around the offset, so it holds up when the current
    /// buffer does not parse. With no retained tree, it falls back to a text scan
    /// and returns the root body context.
    fn resolve(&self, tree: Option<&Fields>, text: &str, offset: usize) -> CursorContext {
        resolve_in_tree(tree, text, offset, self.block_span_covers_body())
    }

    /// Whether a block's span covers its whole body.
    ///
    /// A brace-delimited block (HCL, KDL) spans its body, so its end bounds the
    /// body. A header-only block (a TOML table) spans only its header, so
    /// resolution extends its body to the next sibling or the end of the
    /// enclosing level. The default is `true`.
    fn block_span_covers_body(&self) -> bool {
        true
    }

    /// Renders a field's insert text in the format, reading the field's
    /// `SchemaType` to spell a scalar as the format's `name = value` form or a
    /// block as its block form. `path` is the enclosing block path, which a
    /// header-based format (TOML) uses to qualify a nested block header.
    fn insert_text(&self, field: &SchemaField, path: &[String]) -> String;
}
