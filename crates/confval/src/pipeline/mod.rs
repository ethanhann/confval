//! Span-first provenance: every parsed value carries its exact location in
//! the source text, so diagnostics can point at the offending value rather
//! than the enclosing section.

/// Keyword sets and the [`keyword_enum!`](crate::keyword_enum) macro.
pub mod keyword;
mod lower;
pub mod narrow;

/// Numeric range constraints and the `range_constraint!` macro.
pub mod range;
mod validate;

pub use lower::{Lower, LowerAuto};

pub use validate::{Validate, ValidateNested};
