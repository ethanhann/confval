//! Generating the `Default` impl for a `#[confval(derive_default)]` spec.
//!
//! The value this produces for a field is the value the parser fills when that
//! field is absent from a present block, for a field that declares a parser
//! default. A field the parser would report as missing has no value to derive,
//! so it is a compile error unless the field states one with
//! `#[confval(derive_default = expr)]`. That form fills only the generated
//! impl, so a field the document must set keeps its value here without
//! becoming optional. An absent block and a present block with the field
//! omitted resolve to the same value for every field with a parser default.
//!
//! This module reads the same `FieldShape` and `FieldOptions` the parser is
//! built from. A leaf with a parser default comes from the shared
//! `FieldOptions::default_value`, the one mapping from a `default` attribute to
//! its expression. A leaf with a generated-only value comes from
//! `FieldOptions::derive_default_value`. The first cannot drift from parsing.

use super::options::FieldOptions;
use super::shape::FieldShape;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The `field: value` fragment for one field's `Default`, or an error when the
/// field has no derivable default.
pub(crate) fn field_ctor(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> syn::Result<TokenStream2> {
    match shape {
        FieldShape::Leaf { optional, .. } => leaf_ctor(ident, *optional, options),
        // A string list has no derivable default unless it has a bare
        // `#[confval(default)]`, which the parser reads as the empty list, or a
        // `derive_default` value for the generated impl. A `default = expr` has
        // no meaning here.
        // `reject_unsupported_default` reports it with a better span, and the
        // arm refuses it too, so this match stands correct on its own rather
        // than through call order.
        FieldShape::BareStringList => {
            if matches!(options.default, Some(None)) {
                Ok(quote! { #ident: ::std::vec::Vec::new(), })
            } else if let Some(expr) = &options.derive_default_value {
                Ok(quote! { #ident: #expr, })
            } else {
                Err(no_default_error(ident))
            }
        }
        // A non-optional nested block defaults through a bare `#[confval(default)]`,
        // which the parser reads as `S::default()`. A `default = expr` is
        // refused for the same reason as on the string list. A `derive_default`
        // value is already the child spec, so it is wrapped in `Located`.
        FieldShape::Nested {
            optional: false, ..
        } => {
            if matches!(options.default, Some(None)) {
                Ok(quote! {
                    #ident: ::confval::source::Located::detached(::core::default::Default::default()),
                })
            } else if let Some(expr) = &options.derive_default_value {
                Ok(quote! {
                    #ident: ::confval::source::Located::detached(#expr),
                })
            } else {
                Err(no_default_error(ident))
            }
        }
        // A map has no derivable default unless it has a bare
        // `#[confval(default)]`, which the parser reads as the empty map. A
        // `default = expr` has no meaning here, the same as on the string list.
        // A `derive_default` value is the whole map.
        FieldShape::Map => {
            if matches!(options.default, Some(None)) {
                Ok(quote! { #ident: ::std::collections::BTreeMap::new(), })
            } else if let Some(expr) = &options.derive_default_value {
                Ok(quote! { #ident: #expr, })
            } else {
                Err(no_default_error(ident))
            }
        }
        // The parser fills these when absent, so no declaration is needed.
        FieldShape::OptionalWrappedStringList | FieldShape::Nested { optional: true, .. } => {
            Ok(quote! { #ident: ::core::option::Option::None, })
        }
        // A nested list is empty when omitted unless a `derive_default` value
        // supplies the whole list.
        FieldShape::NestedList { .. } => {
            if let Some(expr) = &options.derive_default_value {
                Ok(quote! { #ident: #expr, })
            } else {
                Ok(quote! { #ident: ::std::vec::Vec::new(), })
            }
        }
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
/// A leaf with a default fills that value, detached from any source location.
/// A non-optional leaf can also supply a `derive_default` value, which builds
/// the same fragment without making the field optional in the parser.
///
/// A non-optional leaf with neither has nothing to derive and is an error.
/// An optional leaf with no default is `None`, which is what the parser leaves
/// for an absent optional field. An optional leaf rejects `derive_default`,
/// because `None` is already the generated value.
fn leaf_ctor(ident: &Ident, optional: bool, options: &FieldOptions) -> syn::Result<TokenStream2> {
    let value = match options.default_value().or_else(|| {
        options
            .derive_default_value
            .as_ref()
            .map(|expr| quote! { #expr })
    }) {
        Some(expr) => quote! { ::confval::source::Located::detached(#expr) },
        None => {
            if optional {
                return Ok(quote! { #ident: ::core::option::Option::None, });
            }
            return Err(no_default_error(ident));
        }
    };
    if optional {
        Ok(quote! { #ident: ::core::option::Option::Some(#value), })
    } else {
        Ok(quote! { #ident: #value, })
    }
}

/// The compile error for a field that `derive_default` cannot fill.
///
/// A bare `#[confval(default)]` resolves every required shape, so it leads. A
/// leaf also accepts `#[confval(default = expr)]`, which the parser rejects on a
/// string list or a nested block, so it is named as the leaf-only variant. A
/// field that must stay required takes `#[confval(derive_default = expr)]`,
/// which fills only the generated impl value.
fn no_default_error(ident: &Ident) -> syn::Error {
    syn::Error::new(
        ident.span(),
        format!(
            "`{ident}` has no derivable default. Add a bare #[confval(default)], \
             or #[confval(default = expr)] for a leaf. Use \
             #[confval(derive_default = expr)] to keep the field required. Or write \
             impl Default by hand."
        ),
    )
}
