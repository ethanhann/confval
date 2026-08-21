//! `#[derive(Spec)]`'s recorded-check half: the per-field fragments for the
//! generated `ValidateNested::validate_recorded`.
//!
//! Where the schema walk in [`schema`](super::schema) records a field's
//! `#[confval(range = ...)]` or `#[confval(keywords = ...)]` constraint for the
//! IR, this walk runs the same constraint during validation, so the attribute is
//! the single source and the author's `Validate` body carries no line for it. A
//! scalar leaf emits a `check_located` call and a string list emits a
//! `check_each_in` call, which reports each bad element at its own span.
//!
//! The walk decides what to emit from the presence of `options.range` or
//! `options.keywords` alone. Which shape may carry which attribute is settled in
//! `spec/schema.rs` when the always-emitted `ToSchema` is generated, so a
//! misplaced attribute is a compile error before this walk runs. Keeping that
//! rule in one generator and reading only attribute presence here keeps the two
//! from drifting on which attribute means what.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
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
///
/// A field with a default gets one more branch. When the value is the default
/// itself, recognized by its detached span and its equality with the declared
/// default, a failed check names the spec's default rather than reporting a
/// config error the operator cannot locate.
pub(crate) fn field_recorded_check(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> Option<TokenStream2> {
    let name = ident.unraw().to_string();

    // The `check_located` call, given the `&Located<T>` value expression and
    // the report expression it writes into. A `range` names a
    // `RangeConstraint` value, a `keywords` names a `keyword_enum!` type whose
    // `keyword_set()` yields the check.
    let call = |value: &TokenStream2, report: &TokenStream2| -> Option<TokenStream2> {
        if let Some(path) = &options.range {
            return Some(quote! { #path.check_located(#value, #name, #report); });
        }
        options
            .keywords
            .as_ref()
            .map(|path| quote! { #path::keyword_set().check_located(#value, #name, #report); })
    };

    if matches!(shape, FieldShape::Leaf { optional: true, .. }) {
        let call = call(&quote! { __value }, &quote! { report })?;
        return Some(quote! {
            if let ::core::option::Option::Some(__value) = &self.#ident {
                #call
            }
        });
    }

    // A list records the constraint for one element, so the check runs through
    // `check_each_in`, which reports each bad element at its own span. Only
    // `keywords` reaches here, because the schema walk refuses `range` and
    // `references` on a list. The bare form is already a slice. The optional
    // form keeps the outer `Located`, so the list is reached through its value.
    //
    // Neither arm carries the defaulted-value branch a required leaf gets
    // below. A list default is always the empty list, so there is no declared
    // value for the constraint to reject.
    let each = |values: &TokenStream2| -> Option<TokenStream2> {
        let path = options.keywords.as_ref()?;
        Some(quote! { #path::keyword_set().check_each_in(#values, #name, report); })
    };
    if matches!(shape, FieldShape::BareStringList) {
        let call = each(&quote! { &self.#ident })?;
        return Some(call);
    }
    if matches!(shape, FieldShape::OptionalWrappedStringList) {
        let call = each(&quote! { &__list.value })?;
        return Some(quote! {
            if let ::core::option::Option::Some(__list) = &self.#ident {
                #call
            }
        });
    }
    let direct = call(&quote! { &self.#ident }, &quote! { report })?;
    let Some(default) = default_expr_typed(shape, options) else {
        return Some(direct);
    };
    let buffered = call(&quote! { &self.#ident }, &quote! { &mut __check })?;
    let prefix = format!("the default for `{name}` fails its recorded constraint: ");
    Some(quote! {
        if self.#ident.span.is_detached() && self.#ident.value == #default {
            let mut __check = ::confval::diagnostic::Report::new();
            #buffered
            for __issue in __check.issues() {
                report
                    .error(::std::format!("{}{}", #prefix, __issue.message))
                    .help(
                        "fix the #[confval(default = ...)] or the recorded constraint"
                            .to_string(),
                    )
                    .emit();
            }
        } else {
            #direct
        }
    })
}

/// The declared default as a typed value expression, or `None` when the field
/// has no default or is not a required leaf. The typed binding pins the
/// expression to the leaf's Rust type, the way the schema walk's default text
/// does.
fn default_expr_typed(shape: &FieldShape, options: &FieldOptions) -> Option<TokenStream2> {
    let FieldShape::Leaf {
        leaf,
        optional: false,
        ..
    } = shape
    else {
        return None;
    };
    let expr = options.default_value()?;
    Some(match leaf {
        Leaf::String => quote! { { let __default: ::std::string::String = #expr; __default } },
        Leaf::Int => quote! { { let __default: i64 = #expr; __default } },
        Leaf::Float => quote! { { let __default: f64 = #expr; __default } },
        Leaf::Bool => quote! { { let __default: bool = #expr; __default } },
        Leaf::PathBuf => quote! { { let __default: ::std::path::PathBuf = #expr; __default } },
    })
}
