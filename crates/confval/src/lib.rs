//! Parse, validate, and lower configuration files, with a source span on
//! every value so a diagnostic reports the exact line and column it came
//! from.
//!
//! The pipeline runs in stages. A format frontend parses text into a
//! format-neutral field model, a spec type parses out of that model, plain
//! validation functions check what the values mean, and lowering narrows the
//! validated spec into the runtime types the rest of the program uses.
//!
//! A field constraint that the derive can record is checked and carried into
//! the schema from one attribute. The attributes name a [`RangeConstraint`],
//! a [`LengthConstraint`], a [`KeywordSet`] through `keyword_enum!`, a type
//! that implements [`Format`], or the [`NON_EMPTY`] and [`UNIQUE`] flags.

#[cfg(feature = "derive")]
pub use confval_derive::{Config, Spec};
/// Collecting and rendering diagnostics: the [`Report`](diagnostic::Report)
/// and its issues.
pub mod diagnostic;
/// The format-neutral field model, the file frontends, and the emitters.
pub mod format;
#[cfg(feature = "layering")]
pub mod layering;
pub mod pipeline;
pub mod schema;
/// Source registration and location: [`SourceMap`](source::SourceMap),
/// [`Span`](source::Span), and [`Located`](source::Located).
pub mod source;

pub use pipeline::format::{AbsolutePath, Format, Ip, Ipv4, Ipv6};
pub use pipeline::keyword::KeywordSet;
pub use pipeline::length::LengthConstraint;
pub use pipeline::non_empty::{NON_EMPTY, NonEmptyConstraint};
pub use pipeline::range::RangeConstraint;
pub use pipeline::unique::{UNIQUE, UniqueConstraint};

/// Implementation detail for the crate's macros. Not part of the public API,
/// and exempt from semver. `keyword_enum!` uses `serde` through this path, so
/// the generated impl is gated on confval's own `serde` feature rather than the
/// caller's dependency graph.
#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "serde")]
    pub use serde;
}

/// The common imports for defining and lowering specs.
///
/// A single `use confval::prelude::*;` pulls the everyday names a spec module
/// uses: the source-location primitives ([`Located`](source::Located),
/// [`Span`](source::Span), [`SourceMap`](source::SourceMap)), the diagnostic
/// [`Report`](diagnostic::Report), the lowering pipeline
/// ([`Lower`](pipeline::Lower), [`Validate`](pipeline::Validate),
/// [`ValidateNested`](pipeline::ValidateNested), the
/// [`ControlFlow`](core::ops::ControlFlow) a `descend` override returns, and
/// the [`narrow`](pipeline::narrow) helpers), the constraint validators
/// ([`KeywordSet`] and its [`keyword_enum!`] macro, [`RangeConstraint`] and its
/// [`range_constraint!`] macro, [`LengthConstraint`] and its
/// [`length_constraint!`] macro, the [`Format`] trait with its built-in
/// types and the [`check_format`](pipeline::check_format) and
/// [`check_each_format`](pipeline::check_each_format) calls, and the
/// [`NON_EMPTY`] and [`UNIQUE`] flags), and, with the `derive` feature, the
/// [`Spec`] and [`Config`] derives. Each validator is exported with its
/// declaration form, so each validated pattern works from one import.
///
/// The write-path trait [`ToFields`](format::ToFields) is in the prelude,
/// because you call `spec.to_fields()` as a method and the trait must be in
/// scope. Its parse counterpart [`FromFields`](format::FromFields) stays out,
/// because a frontend calls it through its module path rather than as a method.
/// The type-level trait [`ToSchema`](schema::ToSchema) is in the prelude for the
/// same reason as `ToFields`: you call `Spec::schema()` and the trait must be in
/// scope.
///
/// Format adapters stay out of the prelude. Use their explicit module path
/// (`confval::format::hcl`). The diagnostic internals
/// ([`Issue`](diagnostic::Issue), [`Severity`](diagnostic::Severity)) and the
/// remaining source types ([`Source`](source::Source),
/// [`SourceId`](source::SourceId)) likewise stay behind their module paths.
pub mod prelude {
    pub use core::ops::ControlFlow;

    pub use crate::diagnostic::Report;
    pub use crate::format::ToFields;
    pub use crate::pipeline::{Lower, Validate, ValidateNested, narrow};
    pub use crate::pipeline::{check_each_format, check_format};
    pub use crate::schema::ToSchema;
    pub use crate::source::{Located, SourceMap, Span};
    pub use crate::{
        AbsolutePath, Format, Ip, Ipv4, Ipv6, KeywordSet, LengthConstraint, NON_EMPTY,
        NonEmptyConstraint, RangeConstraint, UNIQUE, UniqueConstraint, keyword_enum,
        length_constraint, range_constraint,
    };

    #[cfg(feature = "derive")]
    pub use crate::{Config, Spec};
}
