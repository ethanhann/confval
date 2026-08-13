//! The format seam: the [`Frontend`] trait and the [`CursorContext`] its
//! resolution produces.
//!
//! A frontend delegates parsing to `confval`, resolves a byte offset to a cursor
//! context, and renders a field's insert text in its format. Parsing and insert
//! rendering reuse `confval`'s machinery, and the block-structured formats
//! resolve through one shared walk, so each frontend's
//! [`resolve`](Frontend::resolve) is the default.

use confval::diagnostic::Report;
use confval::format::Fields;
use confval::schema::SchemaField;
use confval::source::{SourceId, SourceMap};

use crate::resolve::resolve_in_tree;
use crate::text_scan::resolve_in_text;

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
    /// produce this variant. It is reserved for label completion.
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
    /// when it sits on no token. It is scanned from the current text, so it stays
    /// valid and on the cursor's line even when the buffer does not parse.
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

    /// Parses `text` into the neutral [`Fields`]. The document store holds the
    /// current parse, or `None` when the text does not parse. A throwaway
    /// [`SourceMap`] holds the text, because resolution reads only byte offsets,
    /// which are the same in any map.
    fn parse_tree(&self, text: &str) -> Option<Fields> {
        let mut sources = SourceMap::new();
        let id = sources.add("<buffer>", text);
        let mut report = Report::new();
        self.parse(&sources, id, &mut report)
    }

    /// Resolves a byte offset to the cursor context.
    ///
    /// When `tree` is present, the buffer parsed and resolution walks it, so the
    /// spans align with the text exactly. When it is absent, the buffer did not
    /// parse, so resolution reconstructs the block path and the position kind
    /// from the raw text, whose offsets are always current.
    fn resolve(&self, tree: Option<&Fields>, text: &str, offset: usize) -> CursorContext {
        match tree {
            Some(tree) => resolve_in_tree(tree, text, offset, self.block_span_covers_body()),
            None => resolve_in_text(
                text,
                offset,
                self.block_span_covers_body(),
                self.attribute_uses_equals(),
                self.hash_is_comment(),
            ),
        }
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

    /// Whether an attribute separates its name and value with `=` (HCL, TOML)
    /// rather than whitespace (KDL). The text-based recovery reads this to detect
    /// a value position when the buffer does not parse. The default is `true`.
    fn attribute_uses_equals(&self) -> bool {
        true
    }

    /// Whether `#` starts a line comment (HCL) rather than a value token (KDL
    /// spells booleans `#true`). The text-based recovery reads this when it scans
    /// blocks. The default is `true`.
    fn hash_is_comment(&self) -> bool {
        true
    }

    /// Renders a field's insert text in the format, reading the field's
    /// `SchemaType` to spell a scalar as the format's `name = value` form or a
    /// block as its block form. `path` is the enclosing block path, which a
    /// header-based format (TOML) uses to qualify a nested block header.
    ///
    /// A brace-delimited block insert places a `$0` where the cursor belongs,
    /// inside the body. The completion handler emits it as a snippet tab stop
    /// when the client supports snippets, or removes it otherwise, so the marker
    /// never reaches a buffer literally.
    fn insert_text(&self, field: &SchemaField, path: &[String]) -> String;
}
