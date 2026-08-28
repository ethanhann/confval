//! The value constraints a spec declares and the derive records: keyword
//! sets, numeric ranges, character lengths, formats, the non-empty flag, and
//! the unique flag. Each checker reports at the value's span. A keyword set
//! is declared with `keyword_enum!`, a range with `range_constraint!`, and a
//! length with `length_constraint!`.
//!
//! This module and [`pipeline`](crate::pipeline) do not import each other.
//! A `Validate` impl in the caller's crate is the only place they meet.

/// The `Format` trait, the built-in formats, and the `check_format` calls.
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

pub use format::{check_each_format, check_format, constraint as format_constraint};
pub use non_empty::{NON_EMPTY, NonEmptyConstraint};
pub use unique::{UNIQUE, UniqueConstraint};
