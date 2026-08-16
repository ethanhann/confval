//! The pure request handlers.
//!
//! Each handler is a function of the document, the schema, and, for completion
//! and hover, a resolved cursor context. They hold no state and touch no socket,
//! so a test builds their inputs from a buffer and an offset and calls one
//! directly.

mod code_action;
mod completion;
mod diagnostics;
mod hover;
mod navigation;
mod symbols;

pub use code_action::code_action;
pub use completion::{Cx, completion};
pub use diagnostics::diagnostics;
pub use hover::hover;
pub use navigation::{definition, references};
pub use symbols::document_symbols;
