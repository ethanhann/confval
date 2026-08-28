//! Validation and lowering of a parsed spec. Every parsed value keeps its
//! exact location in the source text, so a diagnostic points at the offending
//! value rather than at the enclosing section. The four stages are the
//! [`Validate`] and [`ValidateNested`] walk, the [`check_references`] pass,
//! the [`narrow`] helpers, and the [`Lower`] step into the runtime form. A
//! `Validate` impl calls the checkers in [`constraints`](crate::constraints).

mod lower;
pub mod narrow;
mod references;
mod validate;

pub use lower::{Lower, LowerAuto};

pub use references::check_references;
#[cfg(feature = "__internal-navigation")]
#[doc(hidden)]
pub use references::{
    ReferenceSite, Scope, declares_labeled_block, scope_labels, visit_references,
};
pub use validate::{Validate, ValidateNested};
