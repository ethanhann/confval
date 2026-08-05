//! Validation and lowering of a parsed spec. Every parsed value carries its
//! exact location in the source text, so a diagnostic points at the offending
//! value rather than at the enclosing section.

/// Keyword sets and the [`keyword_enum!`](crate::keyword_enum) macro.
pub mod keyword;
mod lower;
pub mod narrow;

/// Numeric range constraints and the `range_constraint!` macro.
pub mod range;
mod validate;

pub use lower::{Lower, LowerAuto};

pub use validate::{Validate, ValidateNested};
