//! The legality rules for `#[derive(Spec)]`'s value constraints. They decide
//! which field shape may carry `keywords`, `range`, `length`, `format`, and
//! `references`, and what a legal pair records in the schema. The flag
//! attributes have their rules in [`flags`](super::flags).
//!
//! The rules live here rather than in `options.rs`, because `options.rs`
//! reads the attribute tokens and never classifies the field type. The schema
//! walk calls these functions for every field, so a misplaced attribute is a
//! compile error before the validation walk runs. The validation walk can
//! then read attribute presence alone.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use syn::ext::IdentExt;
use syn::{Ident, Path};

/// The `Option<Constraint>` expression a field records, and the one place a
/// (shape, attribute) pair is judged legal.
///
/// The mutual-exclusion check runs first for every shape, so a field carrying
/// two recording attributes reads that mistake rather than a pairing message
/// about one of them. What each shape can then carry differs. A scalar leaf
/// records any of the five against its leaf type. A string list records
/// `keywords` or `format`, each applied to every element. A map and a nested
/// block record nothing.
pub(crate) fn constraint_tokens(
    shape: &FieldShape,
    options: &FieldOptions,
) -> syn::Result<TokenStream2> {
    let Some(recorded) = one_recording_attribute(options)? else {
        return Ok(quote! { ::core::option::Option::None });
    };
    match shape {
        FieldShape::Leaf { leaf, .. } => leaf_constraint(leaf, recorded),
        FieldShape::BareStringList | FieldShape::OptionalWrappedStringList => match recorded {
            Recorded::Keywords(path) => Ok(keywords_tokens(path)),
            Recorded::Format(path) => Ok(format_tokens(path)),
            Recorded::Range(path) => Err(refused(path, RANGE_REQUIRES, "a list")),
            Recorded::Length(path) => Err(refused(path, LENGTH_REQUIRES, "a list")),
            Recorded::References(block) => Err(refused(block, REFERENCES_REQUIRES, "a list")),
        },
        FieldShape::Nested { .. } | FieldShape::NestedList { .. } | FieldShape::Map => {
            let where_not = "a map or a nested block";
            match recorded {
                Recorded::Keywords(path) => Err(refused(path, KEYWORDS_REQUIRES, where_not)),
                Recorded::Range(path) => Err(refused(path, RANGE_REQUIRES, where_not)),
                Recorded::Length(path) => Err(refused(path, LENGTH_REQUIRES, where_not)),
                Recorded::Format(path) => Err(refused(path, FORMAT_REQUIRES, where_not)),
                Recorded::References(block) => Err(refused(block, REFERENCES_REQUIRES, where_not)),
            }
        }
    }
}

const KEYWORDS_REQUIRES: &str =
    "#[confval(keywords = ...)] requires a String leaf or a string list";
const RANGE_REQUIRES: &str = "#[confval(range = ...)] requires an Int or Float leaf";
const LENGTH_REQUIRES: &str = "#[confval(length = ...)] requires a String leaf";
const FORMAT_REQUIRES: &str = "#[confval(format = ...)] requires a String leaf or a string list";
const REFERENCES_REQUIRES: &str = "#[confval(references = ...)] requires a String leaf";

/// The one recording attribute a field declares.
pub(crate) enum Recorded<'a> {
    Keywords(&'a Path),
    Range(&'a Path),
    Length(&'a Path),
    Format(&'a Path),
    References(&'a Ident),
}

impl Recorded<'_> {
    /// The attribute's own token, the place an error about it points at.
    fn tokens(&self) -> &dyn ToTokens {
        match self {
            Recorded::Keywords(path)
            | Recorded::Range(path)
            | Recorded::Length(path)
            | Recorded::Format(path) => path,
            Recorded::References(block) => block,
        }
    }
}

/// The recording attribute a field declares, or `None` when it declares none.
///
/// Two of them on one field is an error wherever the field sits, because no
/// shape carries two constraints.
pub(crate) fn one_recording_attribute(options: &FieldOptions) -> syn::Result<Option<Recorded<'_>>> {
    let too_many = "a field takes at most one of #[confval(keywords = ...)], \
                    #[confval(range = ...)], #[confval(length = ...)], \
                    #[confval(format = ...)], or #[confval(references = ...)]";
    let mut found: Vec<Recorded<'_>> = Vec::new();
    if let Some(path) = &options.keywords {
        found.push(Recorded::Keywords(path));
    }
    if let Some(path) = &options.range {
        found.push(Recorded::Range(path));
    }
    if let Some(path) = &options.length {
        found.push(Recorded::Length(path));
    }
    if let Some(path) = &options.format {
        found.push(Recorded::Format(path));
    }
    if let Some(block) = &options.references {
        found.push(Recorded::References(block));
    }
    // `FieldOptions` keeps no source order, so the reported attribute follows
    // this fixed order rather than the order the author wrote. The snapshots
    // depend on a deterministic choice.
    if let Some(second) = found.get(1) {
        return Err(syn::Error::new_spanned(second.tokens(), too_many));
    }
    Ok(found.into_iter().next())
}

/// The constraint a scalar leaf records, paired against its leaf type.
fn leaf_constraint(leaf: &Leaf, recorded: Recorded<'_>) -> syn::Result<TokenStream2> {
    match recorded {
        Recorded::Keywords(path) => {
            if !matches!(leaf, Leaf::String) {
                return Err(syn::Error::new_spanned(path, KEYWORDS_REQUIRES));
            }
            Ok(keywords_tokens(path))
        }
        Recorded::Range(path) => {
            if !matches!(leaf, Leaf::Int | Leaf::Float) {
                return Err(syn::Error::new_spanned(path, RANGE_REQUIRES));
            }
            // A float bound renders through `{:?}`, the form the default text
            // uses, so a whole-number bound keeps its `.0` and hover on a
            // float field reads float text.
            let (min, max) = match leaf {
                Leaf::Float => (
                    quote! { ::std::format!("{:?}", #path.min) },
                    quote! { ::std::format!("{:?}", #path.max) },
                ),
                _ => (
                    quote! { ::std::string::ToString::to_string(&#path.min) },
                    quote! { ::std::string::ToString::to_string(&#path.max) },
                ),
            };
            Ok(quote! {
                ::core::option::Option::Some(
                    ::confval::schema::Constraint::range(
                        #min,
                        #max,
                        #path.units,
                        #path.help,
                    ),
                )
            })
        }
        Recorded::Length(path) => {
            if !matches!(leaf, Leaf::String) {
                return Err(syn::Error::new_spanned(path, LENGTH_REQUIRES));
            }
            // A character count has one type, so the bounds pass through as
            // they are and the hover needs no text to parse.
            Ok(quote! {
                ::core::option::Option::Some(
                    ::confval::schema::Constraint::length(
                        #path.min,
                        #path.max,
                        #path.help,
                    ),
                )
            })
        }
        Recorded::Format(path) => {
            if !matches!(leaf, Leaf::String) {
                return Err(syn::Error::new_spanned(path, FORMAT_REQUIRES));
            }
            Ok(format_tokens(path))
        }
        Recorded::References(block) => {
            if !matches!(leaf, Leaf::String) {
                return Err(syn::Error::new_spanned(block, REFERENCES_REQUIRES));
            }
            let block = block.unraw().to_string();
            Ok(quote! {
                ::core::option::Option::Some(
                    ::confval::schema::Constraint::references(#block),
                )
            })
        }
    }
}

/// The `Constraint::keywords` expression reading a `keyword_enum!` type's set.
fn keywords_tokens(path: &Path) -> TokenStream2 {
    quote! {
        ::core::option::Option::Some(
            ::confval::schema::Constraint::keywords(&#path::KEYWORDS),
        )
    }
}

/// The schema record for a `Format` type, built by the pipeline's own
/// `format_constraint` so the name and the check come from the one type.
fn format_tokens(path: &Path) -> TokenStream2 {
    quote! {
        ::core::option::Option::Some(::confval::pipeline::format_constraint::<#path>())
    }
}

/// A refusal naming what the attribute needs and the shape it cannot sit on.
fn refused<T: ToTokens>(at: &T, requires: &str, where_not: &str) -> syn::Error {
    syn::Error::new_spanned(at, format!("{requires}; it cannot apply to {where_not}"))
}
