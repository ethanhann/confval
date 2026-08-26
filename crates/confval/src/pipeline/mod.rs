//! Validation and lowering of a parsed spec. Every parsed value carries its
//! exact location in the source text, so a diagnostic points at the offending
//! value rather than at the enclosing section.

/// Keyword sets and the [`keyword_enum!`](crate::keyword_enum) macro.
pub mod keyword;
mod lower;
pub mod narrow;

/// The `Format` trait, the built-in formats, and the `check_format` calls.
pub mod format;
/// Character length constraints and the `length_constraint!` macro.
pub mod length;
/// The non-empty constraint and the `NON_EMPTY` constant.
pub mod non_empty;
/// Numeric range constraints and the `range_constraint!` macro.
pub mod range;
mod references;
mod validate;

pub use lower::{Lower, LowerAuto};
pub use non_empty::{NON_EMPTY, NonEmptyConstraint};

pub use references::check_references;
#[cfg(feature = "__internal-navigation")]
#[doc(hidden)]
pub use references::{
    ReferenceSite, Scope, declares_labeled_block, scope_labels, visit_references,
};
pub use validate::{Validate, ValidateNested};
