//! The pure request handlers.
//!
//! Each handler is a function of the document, the schema, and a resolved
//! cursor context. They hold no state and use no socket, so a test builds
//! their inputs from a buffer and an offset and calls one directly. A handler
//! taking three or more document inputs takes the [`Cx`] bundle, and a handler
//! needing fewer takes them loose.

mod code_action;
mod completion;
mod diagnostics;
mod hover;
mod navigation;
mod symbols;

pub use code_action::code_action;
pub use completion::{ClientSupport, completion};

use confval::format::Fields;
use confval::schema::Schema;

use crate::frontend::CursorContext;

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
pub use diagnostics::diagnostics;
pub use hover::hover;
pub use navigation::{definition, references};
pub use symbols::{SymbolShape, document_symbols};
