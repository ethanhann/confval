//! The pure request handlers.
//!
//! Each handler is a function of the document, the schema, and a resolved
//! cursor context. They hold no state and use no socket, so a test builds
//! their inputs from a buffer and an offset and calls one directly. A handler
//! that reads the full document context, the schema, the parse, the cursor,
//! and the text, takes the [`Cx`] bundle. A handler that reads a subset takes
//! its inputs loose.

mod code_action;
mod completion;
mod diagnostics;
mod document_link;
mod hover;
mod navigation;
mod symbols;

pub use code_action::code_action;
pub use completion::{ClientSupport, completion};
pub use diagnostics::diagnostics;
pub use document_link::document_links;
pub use hover::hover;
pub use navigation::{definition, references};
pub use symbols::{SymbolShape, document_symbols};

use confval::format::Fields;
use confval::schema::{Schema, SchemaType};

use crate::frontend::{CursorContext, PositionKind};
use crate::walk::schema_at;

/// The document inputs a handler reads: the schema and parse beside the
/// resolved cursor context and the buffer text.
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
    /// The buffer text.
    pub text: &'a str,
}

/// The list a cursor inside a sequence element belongs to, and the level that
/// declares it.
///
/// A sequence element appears on its own line in YAML and inside brackets in the
/// JSON recovery, so resolution reads the element as a body position under the
/// list's own key. A list of strings has no body, so that position is really the
/// value of the list itself. Reading the parent level answers which it is, and
/// answering only for a string list leaves a sequence of blocks resolving to the
/// body position its elements need.
pub(crate) fn string_list_element<'a>(cx: &'a Cx) -> Option<(&'a Schema, &'a str)> {
    if !matches!(cx.ctx.kind, PositionKind::Body) {
        return None;
    }
    let (field, parent_path) = cx.ctx.path.split_last()?;
    let parent = schema_at(cx.schema, parent_path)?;
    let target = parent.fields.iter().find(|entry| entry.name == *field)?;
    matches!(target.ty, SchemaType::StringList { .. }).then_some((parent, field.as_str()))
}
