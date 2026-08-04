//! `#[derive(Spec)]`'s populate half: the `to_fields` and `to_template` walks.
//!
//! `ToFields` is the counterpart of `FromFields` on the write path. Both walks
//! here read a spec instance and build a populated `Fields`, filling every
//! absent defaultable block and detaching every span. This module emits one
//! fragment per field, read off the same `FieldShape` and `FieldOptions` the
//! parser is built from, so the two halves cannot disagree about a field's
//! shape.
//!
//! The two walks are one code path with an `annotate` flag, rather than two.
//! The template walk is a set of deltas on the populated one: it recurses with
//! `to_template`, attaches each field's doc, and renders an absent optional
//! field as a commented entry. Generating them separately would mean writing
//! every field shape twice, and the two copies could then disagree.
//!
//! The `impl ToFields` these fragments go into is assembled by the `to_fields`
//! sibling module, which also takes the `to_source_fields` walk from
//! `source_view`.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;
use syn::ext::IdentExt;

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
    annotate: bool,
) -> TokenStream2 {
    // A field whose config key is a Rust keyword is a raw identifier, so strip
    // the `r#` before it becomes the emitted key name.
    let name = ident.unraw().to_string();
    // The template walk recurses with `to_template`, so a nested block's own
    // children carry their comments too. The plain walk recurses with
    // `to_fields`.
    let recurse = if annotate {
        quote! { to_template }
    } else {
        quote! { to_fields }
    };
    // The comment to attach to each field, or nothing on the plain walk or for a
    // field with no doc. Appending an empty token leaves the field unchanged.
    let doc = match (annotate, &options.doc) {
        (true, Some(text)) => {
            quote! { .with_doc(::core::option::Option::Some(#text.to_string())) }
        }
        _ => quote! {},
    };
    // A block whose embedding field has no doc falls back to the child type's
    // own doc at runtime, read through `spec_doc`. The receiver differs per
    // arm, so each nested arm supplies the expression its block recurses on.
    let nested_doc = |receiver: TokenStream2| -> TokenStream2 {
        match (annotate, &options.doc) {
            (false, _) => quote! {},
            (true, Some(_)) => doc.clone(),
            (true, None) => {
                quote! { .with_doc(::confval::format::ToFields::spec_doc(#receiver)) }
            }
        }
    };
    match shape {
        FieldShape::Leaf { leaf, optional } => {
            if *optional {
                let scalar = leaf_scalar(leaf, &quote! { __value });
                // The template shows an absent optional leaf as a commented
                // entry: the attribute default when one exists, a type-shaped
                // zero value otherwise, with the doc comment above it.
                let absent = if annotate {
                    let placeholder = options.default_value().unwrap_or_else(|| zero_value(leaf));
                    let placeholder_scalar = leaf_scalar(leaf, &quote! { __placeholder });
                    quote! {
                        else {
                            let __placeholder =
                                ::confval::source::Located::detached(#placeholder);
                            __items.push(::confval::format::Field::detached_value(
                                #name,
                                ::confval::format::Value::detached(
                                    ::confval::format::ValueKind::Scalar(#placeholder_scalar),
                                ),
                            )#doc.as_commented());
                        }
                    }
                } else {
                    quote! {}
                };
                quote! {
                    if let ::core::option::Option::Some(__value) = &self.#ident {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(
                                ::confval::format::ValueKind::Scalar(#scalar),
                            ),
                        )#doc);
                    } #absent
                }
            } else {
                let scalar = leaf_scalar(leaf, &quote! { self.#ident });
                quote! {
                    __items.push(::confval::format::Field::detached_value(
                        #name,
                        ::confval::format::Value::detached(
                            ::confval::format::ValueKind::Scalar(#scalar),
                        ),
                    )#doc);
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
                )#doc);
            }
        }
        FieldShape::OptionalWrappedStringList => {
            let element = string_element();
            let absent = if annotate {
                quote! {
                    else {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(::confval::format::ValueKind::Seq(
                                ::std::vec::Vec::new(),
                            )),
                        )#doc.as_commented());
                    }
                }
            } else {
                quote! {}
            };
            quote! {
                if let ::core::option::Option::Some(__list) = &self.#ident {
                    __items.push(::confval::format::Field::detached_value(
                        #name,
                        ::confval::format::Value::detached(::confval::format::ValueKind::Seq(
                            __list.value.iter().map(#element).collect(),
                        )),
                    )#doc);
                } #absent
            }
        }
        FieldShape::Nested { optional, spec_ty } => {
            if !*optional {
                let doc = nested_doc(quote! { &self.#ident.value });
                quote! {
                    __items.push(::confval::format::Field::detached_block(
                        #name,
                        ::confval::format::ToFields::#recurse(&self.#ident.value),
                    )#doc);
                }
            } else if options.default.is_some() {
                // The populate marker: fill an absent block from `S::default()`,
                // spelling the inner type so the `Fields` return can infer it.
                let spec_ty = &**spec_ty;
                let child_doc = nested_doc(quote! { &__child.value });
                let filled_doc = nested_doc(quote! { &__filled });
                quote! {
                    match &self.#ident {
                        ::core::option::Option::Some(__child) => {
                            __items.push(::confval::format::Field::detached_block(
                                #name,
                                ::confval::format::ToFields::#recurse(&__child.value),
                            )#child_doc);
                        }
                        ::core::option::Option::None => {
                            let __filled: #spec_ty = ::core::default::Default::default();
                            __items.push(::confval::format::Field::detached_block(
                                #name,
                                ::confval::format::ToFields::#recurse(&__filled),
                            )#filled_doc);
                        }
                    }
                }
            } else {
                let doc = nested_doc(quote! { &__child.value });
                // The template shows an absent unmarked block as a commented
                // empty block. Its contents need an instance the field cannot
                // provide, so the entry shows existence alone. The doc falls
                // back to the type's own, read without an instance.
                let absent = if annotate {
                    let absent_doc = absent_block_doc(options, spec_ty);
                    quote! {
                        else {
                            __items.push(::confval::format::Field::detached_block(
                                #name,
                                ::confval::format::Fields::detached(::std::vec::Vec::new()),
                            )#absent_doc.as_commented());
                        }
                    }
                } else {
                    quote! {}
                };
                quote! {
                    if let ::core::option::Option::Some(__child) = &self.#ident {
                        __items.push(::confval::format::Field::detached_block(
                            #name,
                            ::confval::format::ToFields::#recurse(&__child.value),
                        )#doc);
                    } #absent
                }
            }
        }
        FieldShape::NestedList { spec_ty } => {
            let doc = nested_doc(quote! { &__child.value });
            // The hint for an empty list is the model's nested-list shape, a
            // sequence of one empty map, so each emitter renders its own
            // repeated-block spelling. TOML can tell it from a single block.
            let absent = if annotate {
                let absent_doc = absent_block_doc(options, spec_ty);
                quote! {
                    if self.#ident.is_empty() {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(::confval::format::ValueKind::Seq(
                                ::std::vec::Vec::from([::confval::format::Value::detached(
                                    ::confval::format::ValueKind::Map(
                                        ::confval::format::Fields::detached(
                                            ::std::vec::Vec::new(),
                                        ),
                                    ),
                                )]),
                            )),
                        )#absent_doc.as_commented());
                    }
                }
            } else {
                quote! {}
            };
            quote! {
                for __child in &self.#ident {
                    __items.push(::confval::format::Field::detached_block(
                        #name,
                        ::confval::format::ToFields::#recurse(&__child.value),
                    )#doc);
                }
                #absent
            }
        }
    }
}

/// The doc tokens for a commented entry that has no instance to ask: the
/// field's own doc, or the type's doc read through `type_doc`.
fn absent_block_doc(options: &FieldOptions, spec_ty: &syn::Type) -> TokenStream2 {
    match &options.doc {
        Some(text) => quote! { .with_doc(::core::option::Option::Some(#text.to_string())) },
        None => quote! {
            .with_doc(<#spec_ty as ::confval::format::ToFields>::type_doc())
        },
    }
}

/// The placeholder a commented entry shows for a leaf with no attribute
/// default: a type-shaped zero value the operator overwrites when
/// uncommenting.
fn zero_value(leaf: &Leaf) -> TokenStream2 {
    match leaf {
        Leaf::String => quote! { ::std::string::String::new() },
        Leaf::Int => quote! { 0i64 },
        Leaf::Float => quote! { 0.0f64 },
        Leaf::Bool => quote! { false },
        Leaf::PathBuf => quote! { ::std::path::PathBuf::new() },
    }
}

/// The `Scalar` expression for a leaf value, the mirror of `leaf_parser`.
///
/// `located` names the `Located<T>` to read, either `self.<field>` for a
/// required leaf or the `Some` binding for an optional one. `PathBuf` has no
/// scalar of its own, so it emits as a string through `to_string_lossy`, the
/// one lossy leaf. An `HclInt` field is an `i64` alias, so it emits `Scalar::Int`
/// with no conversion.
pub(super) fn leaf_scalar(leaf: &Leaf, located: &TokenStream2) -> TokenStream2 {
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
