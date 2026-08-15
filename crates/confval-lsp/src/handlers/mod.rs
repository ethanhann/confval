//! The pure request handlers.
//!
//! Each handler is a function of the document, the schema, and, for completion
//! and hover, a resolved cursor context. They hold no state and touch no socket,
//! so a test builds their inputs from a buffer and an offset and calls one
//! directly.

mod completion;
mod diagnostics;
mod hover;

pub use completion::{Cx, completion};
pub use diagnostics::diagnostics;
pub use hover::hover;
