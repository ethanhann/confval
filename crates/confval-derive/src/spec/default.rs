//! Generating the `Default` impl for a `#[confval(derive_default)]` spec.
//!
//! The value this produces for a field is the value the parser fills when that
//! field is absent from a present block. A field the parser would report as
//! missing has no value to derive, so it is a compile error. The two cases then
//! agree: a block that is absent entirely and a block present with a field
//! omitted resolve to the same value.
//!
//! This module reads the same `FieldShape` and `FieldOptions` the parser is
//! built from, so the generated default cannot drift from what parsing fills.

use super::options::FieldOptions;
use super::shape::FieldShape;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Field, Ident};

/// The `field: value` fragment for one field's `Default`, or an error when the
/// field has no derivable default.
pub(crate) fn field_ctor(
    field: &Field,
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> syn::Result<TokenStream2> {
    match shape {
        FieldShape::Leaf { optional, .. } => leaf_ctor(field, ident, *optional, options),
        // A string list is required unless it carries a bare `#[confval(default)]`,
        // which the parser reads as the empty list.
        FieldShape::BareStringList => match options.default {
            Some(_) => Ok(quote! { #ident: ::std::vec::Vec::new(), }),
            None => Err(no_default_error(field, ident)),
        },
        // A non-optional nested block defaults through a bare `#[confval(default)]`,
        // which the parser reads as `S::default()`. A `default = expr` on this
        // shape is already rejected by the parser.
        FieldShape::Nested { optional: false } => match options.default {
            Some(_) => Ok(quote! {
                #ident: ::confval::source::Located::detached(::core::default::Default::default()),
            }),
            None => Err(no_default_error(field, ident)),
        },
        // The parser fills these when absent, so no declaration is needed.
        FieldShape::OptionalWrappedStringList | FieldShape::Nested { optional: true } => {
            Ok(quote! { #ident: ::core::option::Option::None, })
        }
        FieldShape::NestedList => Ok(quote! { #ident: ::std::vec::Vec::new(), }),
    }
}

/// Assembles the field fragments into the generated `impl Default`.
pub(crate) fn default_impl(name: &Ident, ctors: &[TokenStream2]) -> TokenStream2 {
    quote! {
        impl ::core::default::Default for #name {
            fn default() -> Self {
                Self {
                    #(#ctors)*
                }
            }
        }
    }
}

/// The default fragment for a leaf field.
///
/// A leaf with a default fills that value, detached from any source location. A
/// non-optional leaf with no default has nothing to derive and is an error. An
/// optional leaf with no default is `None`, which is what the parser leaves for
/// an absent optional field.
fn leaf_ctor(
    field: &Field,
    ident: &Ident,
    optional: bool,
    options: &FieldOptions,
) -> syn::Result<TokenStream2> {
    let value = match &options.default {
        Some(Some(expr)) => quote! { ::confval::source::Located::detached(#expr) },
        Some(None) => {
            quote! { ::confval::source::Located::detached(::core::default::Default::default()) }
        }
        None => {
            if optional {
                return Ok(quote! { #ident: ::core::option::Option::None, });
            }
            return Err(no_default_error(field, ident));
        }
    };
    if optional {
        Ok(quote! { #ident: ::core::option::Option::Some(#value), })
    } else {
        Ok(quote! { #ident: #value, })
    }
}

/// The compile error for a field that `derive_default` cannot fill.
fn no_default_error(field: &Field, ident: &Ident) -> syn::Error {
    syn::Error::new(
        field.span(),
        format!(
            "`{ident}` has no derivable default; add #[confval(default = ...)] \
             or write impl Default by hand"
        ),
    )
}
