//! The generated half of validation: one `validate_all` call per
//! `#[confval(nested)]` field, emitted as an
//! `impl confval::pipeline::ValidateNested` alongside the parser.
//!
//! The rules themselves are not generated. They live in a `Validate` impl the
//! author writes, because only the author knows which values are legal.
//! Descending into child blocks is derived from the struct definition rather
//! than maintained by hand.
//!
//! The traversal reads the field shapes the parser already classified, so the
//! three nested shapes each get the call they need. Non-nested fields are
//! skipped.

use super::shape::FieldShape;

use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;

/// The call that descends into one nested field, or `None` for a field that
/// holds no child spec.
///
/// Each arm calls `validate_all` rather than `validate`. The child therefore
/// runs its own rules and then its own traversal. That is what carries the
/// walk down an arbitrarily deep spec tree from a single call at the root.
pub(crate) fn nested_visit(shape: &FieldShape, ident: &Ident) -> Option<TokenStream2> {
    match shape {
        FieldShape::Nested {
            optional: false, ..
        } => Some(quote! {
            ::confval::pipeline::Validate::validate_all(&self.#ident.value, report);
        }),
        FieldShape::Nested { optional: true, .. } => Some(quote! {
            if let ::core::option::Option::Some(__child) = &self.#ident {
                ::confval::pipeline::Validate::validate_all(&__child.value, report);
            }
        }),
        FieldShape::NestedList { .. } => Some(quote! {
            for __child in &self.#ident {
                ::confval::pipeline::Validate::validate_all(&__child.value, report);
            }
        }),
        FieldShape::Leaf { .. }
        | FieldShape::BareStringList
        | FieldShape::OptionalWrappedStringList => None,
    }
}

/// Builds the `ValidateNested` impl from the visits collected while walking the
/// struct's fields.
///
/// A struct with no nested fields still gets an impl, with an empty body.
/// `Validate::validate_all` requires the impl. Omitting it for the leaf case
/// would make the entry point unavailable where the tree ends.
pub(crate) fn validate_nested_impl(name: &Ident, visits: &[TokenStream2]) -> TokenStream2 {
    let report = if visits.is_empty() {
        quote! { _report }
    } else {
        quote! { report }
    };
    quote! {
        impl ::confval::pipeline::ValidateNested for #name {
            fn validate_nested(&self, #report: &mut ::confval::diagnostic::Report) {
                #(#visits)*
            }
        }
    }
}
