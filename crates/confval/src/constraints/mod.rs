//! The value constraints a spec declares and the derive records: keyword
//! sets, numeric ranges, character lengths, formats, the non-empty flag, and
//! the unique flag. Each checker reports at the value's span. Each kind with
//! a declaration form has a macro: `keyword_enum!`, `range_constraint!`, and
//! `length_constraint!`.
//!
//! A range and a length describe themselves for the schema through
//! `constraint()`. A keyword set's record is
//! `Constraint::keywords(&T::KEYWORDS)`, read from the same table
//! `keyword_set()` reads. A format's record is `format_constraint::<T>()`.
//!
//! This module and [`pipeline`](crate::pipeline) do not import each other.
//! A `Validate` impl in the caller's crate calls into both.

/// The `Format` trait, the built-in formats, and the `check_format`,
/// `check_format_path`, and `check_each_format` calls.
pub mod format;
/// Keyword sets and the [`keyword_enum!`](crate::keyword_enum) macro.
pub mod keyword;
/// Character length constraints and the `length_constraint!` macro.
pub mod length;
/// The non-empty constraint and the `NON_EMPTY` constant.
pub mod non_empty;
/// Numeric range constraints and the `range_constraint!` macro.
pub mod range;
/// The unique constraint and the `UNIQUE` constant.
pub mod unique;

pub use format::{
    Format, check_each_format, check_format, check_format_path, constraint as format_constraint,
};
pub use keyword::KeywordSet;
pub use length::LengthConstraint;
pub use non_empty::{NON_EMPTY, NonEmptyConstraint};
pub use range::RangeConstraint;
pub use unique::{UNIQUE, UniqueConstraint};
