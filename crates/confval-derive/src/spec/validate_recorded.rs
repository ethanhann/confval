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
//! `#[confval(non_empty)]` is a separate flag, not a value constraint. It
//! emits alongside the constraint call. On a string leaf it calls
//! `NON_EMPTY.check_located`. On a string list it calls both
//! `NON_EMPTY.check_list` (list-level) and `NON_EMPTY.check_each`
//! (per-element).
//!
//! The walk decides what to emit from the presence of `options.range`,
//! `options.keywords`, or `options.non_empty`. Which shape may carry which
//! attribute is settled in `spec/schema.rs` when the always-emitted `ToSchema`
//! is generated, so a misplaced attribute is a compile error before this walk
//! runs.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;
use syn::ext::IdentExt;

/// The check fragment for one field's recorded constraint and non_empty flag.
/// Returns `None` when the field carries no recorded attribute and no
/// `non_empty` flag.
pub(crate) fn field_recorded_check(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> Option<TokenStream2> {
    let name = ident.unraw().to_string();
    let constraint = constraint_fragment(ident, shape, options, &name);
    let non_empty = non_empty_fragment(ident, shape, options, &name);
    match (constraint, non_empty) {
        (Some(c), Some(n)) => Some(quote! { #c #n }),
        (Some(c), None) => Some(c),
        (None, Some(n)) => Some(n),
        (None, None) => None,
    }
}

/// The value-constraint fragment: `range` or `keywords`.
fn constraint_fragment(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
    name: &str,
) -> Option<TokenStream2> {
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

    let check_each_call = |values: &TokenStream2| -> Option<TokenStream2> {
        let path = options.keywords.as_ref()?;
        Some(quote! { #path::keyword_set().check_each_in(#values, #name, report); })
    };
    if matches!(shape, FieldShape::BareStringList) {
        return check_each_call(&quote! { &self.#ident });
    }
    if matches!(shape, FieldShape::OptionalWrappedStringList) {
        let call = check_each_call(&quote! { &__list.value })?;
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

/// The `non_empty` fragment. On a string leaf it calls `check_located`. On a
/// string list it calls both `check_list` (list-level) and `check_each`
/// (per-element).
fn non_empty_fragment(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
    name: &str,
) -> Option<TokenStream2> {
    if !options.non_empty {
        return None;
    }
    match shape {
        FieldShape::Leaf {
            leaf: Leaf::String,
            optional: false,
            ..
        } => Some(quote! {
            ::confval::pipeline::NON_EMPTY.check_located(&self.#ident, #name, report);
        }),
        FieldShape::Leaf {
            leaf: Leaf::String,
            optional: true,
            ..
        } => Some(quote! {
            if let ::core::option::Option::Some(__value) = &self.#ident {
                ::confval::pipeline::NON_EMPTY.check_located(__value, #name, report);
            }
        }),
        FieldShape::BareStringList => Some(quote! {
            ::confval::pipeline::NON_EMPTY.check_each(&self.#ident, #name, report);
        }),
        FieldShape::OptionalWrappedStringList => Some(quote! {
            if let ::core::option::Option::Some(__list) = &self.#ident {
                ::confval::pipeline::NON_EMPTY.check_list(
                    &__list.value, #name, __list.span, report,
                );
                ::confval::pipeline::NON_EMPTY.check_each(&__list.value, #name, report);
            }
        }),
        _ => None,
    }
}

/// The declared default as a typed value expression, or `None` when the field
/// has no default or is not a required leaf.
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
