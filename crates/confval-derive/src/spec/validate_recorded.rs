//! `#[derive(Spec)]`'s recorded-check half: the per-field fragments for the
//! generated `ValidateNested::validate_recorded`.
//!
//! Where the schema walk in [`schema`](super::schema) records a field's
//! `#[confval(range = ...)]` or `#[confval(keywords = ...)]` constraint for the
//! IR, this walk runs the same constraint during validation. It emits one
//! `check_located` call per recorded field, so the attribute is the single
//! source and the author's `Validate` body carries no line for it.
//!
//! The walk decides what to emit from the presence of `options.range` or
//! `options.keywords` alone. The leaf-type pairing, that `keywords` needs a
//! `String` leaf and `range` needs an `Int` or `Float` leaf, already ran in
//! `spec/schema.rs` when the always-emitted `ToSchema` was generated, so a
//! mispaired attribute is a compile error before this walk runs. Keeping the
//! pairing in one generator and reading only attribute presence here keeps the
//! two from drifting on which attribute means what.

use super::options::FieldOptions;
use super::shape::FieldShape;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;
use syn::ext::IdentExt;

/// The check fragment for one field's recorded constraint, or `None` when the
/// field carries neither a `range` nor a `keywords` attribute.
///
/// A required leaf checks `&self.field` directly. An optional leaf checks only
/// when present, through `if let Some`. The field name is the config-key string,
/// derived through the same `unraw` form the schema walk uses, so a raw
/// identifier matches the name the manual call passed.
pub(crate) fn field_recorded_check(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> Option<TokenStream2> {
    let name = ident.unraw().to_string();

    // The `check_located` call, given the `&Located<T>` value expression. A
    // `range` names a `RangeConstraint` value, a `keywords` names a
    // `keyword_enum!` type whose `keyword_set()` yields the check.
    let call = |value: &TokenStream2| -> Option<TokenStream2> {
        if let Some(path) = &options.range {
            return Some(quote! { #path.check_located(#value, #name, report); });
        }
        options
            .keywords
            .as_ref()
            .map(|path| quote! { #path::keyword_set().check_located(#value, #name, report); })
    };

    if matches!(shape, FieldShape::Leaf { optional: true, .. }) {
        let call = call(&quote! { __value })?;
        Some(quote! {
            if let ::core::option::Option::Some(__value) = &self.#ident {
                #call
            }
        })
    } else {
        let call = call(&quote! { &self.#ident })?;
        Some(call)
    }
}
