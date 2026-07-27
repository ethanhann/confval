//! `#[derive(Spec)]`'s write half: generating `impl ToFields`.
//!
//! `ToFields` is the counterpart of `FromFields` on the write path. It walks a
//! spec instance and builds a populated `Fields`, filling every absent
//! defaultable block and detaching every span. This module emits one fragment
//! per field, read off the same `FieldShape` and `FieldOptions` the parser is
//! built from, so the two halves cannot disagree about a field's shape.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;

/// The fragment that pushes one field onto the populated level's item vector,
/// or nothing when the field has no value to show.
///
/// A required field always pushes. An optional field pushes only when it is
/// present, except an optional nested block carrying the populate marker
/// `#[confval(nested, default)]`, which fills an absent block from the inner
/// type's `Default`.
pub(crate) fn field_emit(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> TokenStream2 {
    let name = ident.to_string();
    match shape {
        FieldShape::Leaf { leaf, optional } => {
            if *optional {
                let scalar = leaf_scalar(leaf, &quote! { __value });
                quote! {
                    if let ::core::option::Option::Some(__value) = &self.#ident {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(
                                ::confval::format::ValueKind::Scalar(#scalar),
                            ),
                        ));
                    }
                }
            } else {
                let scalar = leaf_scalar(leaf, &quote! { self.#ident });
                quote! {
                    __items.push(::confval::format::Field::detached_value(
                        #name,
                        ::confval::format::Value::detached(
                            ::confval::format::ValueKind::Scalar(#scalar),
                        ),
                    ));
                }
            }
        }
        FieldShape::BareStringList => {
            let element = string_element();
            quote! {
                __items.push(::confval::format::Field::detached_value(
                    #name,
                    ::confval::format::Value::detached(::confval::format::ValueKind::Seq(
                        self.#ident.iter().map(#element).collect(),
                    )),
                ));
            }
        }
        FieldShape::OptionalWrappedStringList => {
            let element = string_element();
            quote! {
                if let ::core::option::Option::Some(__list) = &self.#ident {
                    __items.push(::confval::format::Field::detached_value(
                        #name,
                        ::confval::format::Value::detached(::confval::format::ValueKind::Seq(
                            __list.value.iter().map(#element).collect(),
                        )),
                    ));
                }
            }
        }
        FieldShape::Nested { optional, spec_ty } => {
            if !*optional {
                quote! {
                    __items.push(::confval::format::Field::detached_block(
                        #name,
                        ::confval::format::ToFields::to_fields(&self.#ident.value),
                    ));
                }
            } else if options.default.is_some() {
                // The populate marker: fill an absent block from `S::default()`,
                // spelling the inner type so the `Fields` return can infer it.
                let spec_ty = &**spec_ty;
                quote! {
                    match &self.#ident {
                        ::core::option::Option::Some(__child) => {
                            __items.push(::confval::format::Field::detached_block(
                                #name,
                                ::confval::format::ToFields::to_fields(&__child.value),
                            ));
                        }
                        ::core::option::Option::None => {
                            let __filled: #spec_ty = ::core::default::Default::default();
                            __items.push(::confval::format::Field::detached_block(
                                #name,
                                ::confval::format::ToFields::to_fields(&__filled),
                            ));
                        }
                    }
                }
            } else {
                quote! {
                    if let ::core::option::Option::Some(__child) = &self.#ident {
                        __items.push(::confval::format::Field::detached_block(
                            #name,
                            ::confval::format::ToFields::to_fields(&__child.value),
                        ));
                    }
                }
            }
        }
        FieldShape::NestedList => quote! {
            for __child in &self.#ident {
                __items.push(::confval::format::Field::detached_block(
                    #name,
                    ::confval::format::ToFields::to_fields(&__child.value),
                ));
            }
        },
    }
}

/// Assembles the field fragments into the generated `impl ToFields`.
///
/// A struct with no fields declares the item vector without `mut`, so the
/// generated impl carries no unused-mut warning under `-D warnings`.
pub(crate) fn to_fields_impl(name: &Ident, emits: &[TokenStream2]) -> TokenStream2 {
    let items_decl = if emits.is_empty() {
        quote! { let __items = ::std::vec::Vec::new(); }
    } else {
        quote! { let mut __items = ::std::vec::Vec::new(); }
    };
    quote! {
        impl ::confval::format::ToFields for #name {
            fn to_fields(&self) -> ::confval::format::Fields {
                #items_decl
                #(#emits)*
                ::confval::format::Fields::detached(__items)
            }
        }
    }
}

/// The `Scalar` expression for a leaf value, the mirror of `leaf_parser`.
///
/// `located` names the `Located<T>` to read, either `self.<field>` for a
/// required leaf or the `Some` binding for an optional one. `PathBuf` has no
/// scalar of its own, so it emits as a string through `to_string_lossy`, the
/// one lossy leaf. An `HclInt` field is an `i64` alias, so it emits `Scalar::Int`
/// with no conversion.
fn leaf_scalar(leaf: &Leaf, located: &TokenStream2) -> TokenStream2 {
    match leaf {
        Leaf::String => quote! { ::confval::format::Scalar::String(#located.value.clone()) },
        Leaf::Int => quote! { ::confval::format::Scalar::Int(#located.value) },
        Leaf::Float => quote! { ::confval::format::Scalar::Float(#located.value) },
        Leaf::Bool => quote! { ::confval::format::Scalar::Bool(#located.value) },
        Leaf::PathBuf => quote! {
            ::confval::format::Scalar::String(#located.value.to_string_lossy().into_owned())
        },
    }
}

/// The closure that maps one `Located<String>` element to a detached string
/// `Value`, shared by the required and optional string-list arms.
fn string_element() -> TokenStream2 {
    quote! {
        |__element| ::confval::format::Value::detached(::confval::format::ValueKind::Scalar(
            ::confval::format::Scalar::String(__element.value.clone()),
        ))
    }
}
