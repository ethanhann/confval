//! The generated half of validation: one `validate_all` call per
//! `#[confval(nested)]` field, emitted as an
//! `impl confval::pipeline::ValidateNested` alongside the parser.
//!
//! The rules themselves are not generated. They are in a `Validate` impl the
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
/// runs its own rules and then its own traversal. That is what takes the
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
        // A string-valued map holds no child spec to descend into, so a
        // per-entry rule stays in the author's own `Validate` impl.
        FieldShape::Leaf { .. }
        | FieldShape::BareStringList
        | FieldShape::OptionalWrappedStringList
        | FieldShape::Map => None,
    }
}

/// Builds the `ValidateNested` impl from the visits and the recorded-constraint
/// checks collected while walking the struct's fields.
///
/// A struct with no nested fields still gets an impl, with an empty
/// `validate_nested` body. `Validate::validate_all` requires the impl. Omitting
/// it for the leaf case would make the entry point unavailable where the tree
/// ends.
///
/// `validate_recorded` is overridden only when the struct has a recorded field.
/// A struct with none uses the trait's default empty body, so the derive emits
/// no dead override.
pub(crate) fn validate_nested_impl(
    name: &Ident,
    visits: &[TokenStream2],
    recorded: &[TokenStream2],
) -> TokenStream2 {
    let nested_report = if visits.is_empty() {
        quote! { _report }
    } else {
        quote! { report }
    };
    let recorded_method = if recorded.is_empty() {
        quote! {}
    } else {
        quote! {
            fn validate_recorded(&self, report: &mut ::confval::diagnostic::Report) {
                #(#recorded)*
            }
        }
    };
    quote! {
        impl ::confval::pipeline::ValidateNested for #name {
            fn validate_nested(&self, #nested_report: &mut ::confval::diagnostic::Report) {
                #(#visits)*
            }

            #recorded_method
        }
    }
}
