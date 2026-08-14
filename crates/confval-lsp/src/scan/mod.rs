//! Raw-text scanners that reconstruct a
//! [`CursorContext`](crate::frontend::CursorContext) from the buffer text.
//!
//! [`text`] recovers the brace and header formats, [`json`] the object-and-key
//! format, and [`yaml`] reads indentation in both parse states. The walk over
//! the parsed tree that serves a clean buffer is in
//! [`resolve`](crate::resolve).

mod json;
mod text;
mod yaml;

pub(crate) use text::{is_identifier, resolve_in_text, skip_string};
pub(crate) use yaml::resolve_in_yaml;
