//! `#[derive(Spec)]`'s source-view half: generating `impl ToFields`'s
//! `to_source_fields`.
//!
//! Where the populate walk fills every default and detaches every span, this
//! walk emits only the fields the source set and keeps their spans. One rule
//! decides every shape. A value is emitted when its `Located` span is attached.
//! A filled default keeps the detached sentinel and is omitted. Each
//! emitted field is built detached and then located with `Field::at`, so a
//! location-aware consumer keeps the source positions. The name span and the
//! level container stay detached, because a spec supplies neither. A bare
//! string list holds no span of its own, so its elements decide it and it is
//! emitted detached.
//!
//! Fragments are read off the same `FieldShape` the parser and the populate
//! walk are built from, so the three cannot disagree about a field's shape.

use super::populate::leaf_scalar;
use super::shape::FieldShape;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;
use syn::ext::IdentExt;

/// The fragment that pushes one field onto the source-view level's item vector
/// when the source set it, or nothing when it did not.
///
/// The shape is the only input. No `#[confval(...)]` option changes what the
/// source view emits, because the span decides it.
pub(crate) fn field_source_emit(ident: &Ident, shape: &FieldShape) -> TokenStream2 {
    // A field whose config key is a Rust keyword is a raw identifier, so strip
    // the `r#` before it becomes the emitted key name.
    let name = ident.unraw().to_string();
    match shape {
        FieldShape::Leaf { leaf, optional } => {
            if *optional {
                let scalar = leaf_scalar(leaf, &quote! { __value });
                quote! {
                    if let ::core::option::Option::Some(__value) = &self.#ident {
                        if !__value.span.is_detached() {
                            __items.push(::confval::format::Field::detached_value(
                                #name,
                                ::confval::format::Value::detached(
                                    ::confval::format::ValueKind::Scalar(#scalar),
                                ),
                            ).at(__value.span));
                        }
                    }
                }
            } else {
                let scalar = leaf_scalar(leaf, &quote! { self.#ident });
                quote! {
                    if !self.#ident.span.is_detached() {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(
                                ::confval::format::ValueKind::Scalar(#scalar),
                            ),
                        ).at(self.#ident.span));
                    }
                }
            }
        }
        // A bare list holds no wrapper span, so its elements keep the only
        // locations it has. An element whose span is detached was never written
        // by a source, so it is dropped, and a list with nothing left is
        // omitted. The field and the sequence container are detached, because
        // no wrapper location exists. A source-written empty list is therefore
        // indistinguishable from an absent one.
        FieldShape::BareStringList => {
            let element = spanned_string_element();
            quote! {
                {
                    let __elements: ::std::vec::Vec<::confval::format::Value> = self
                        .#ident
                        .iter()
                        .filter(|__element| !__element.span.is_detached())
                        .map(#element)
                        .collect();
                    if !__elements.is_empty() {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(
                                ::confval::format::ValueKind::Seq(__elements),
                            ),
                        ));
                    }
                }
            }
        }
        // The wrapped list keeps its own span, so a source-written empty list
        // survives while an absent field is omitted. `at` gives the list its
        // wrapper span and leaves each element's own span unchanged.
        FieldShape::OptionalWrappedStringList => {
            let element = spanned_string_element();
            quote! {
                if let ::core::option::Option::Some(__list) = &self.#ident {
                    if !__list.span.is_detached() {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(
                                ::confval::format::ValueKind::Seq(
                                    __list.value.iter().map(#element).collect(),
                                ),
                            ),
                        ).at(__list.span));
                    }
                }
            }
        }
        // A required or defaulted `Located<S>` block. A source-written block has
        // an attached span, and a default fill keeps the detached sentinel, so
        // the same span check decides both. The contents filter recursively, so
        // a block the source wrote with every inner field defaulted emits empty.
        FieldShape::Nested {
            optional: false, ..
        } => {
            quote! {
                if !self.#ident.span.is_detached() {
                    __items.push(::confval::format::Field::detached_block(
                        #name,
                        ::confval::format::ToFields::to_source_fields(&self.#ident.value),
                    ).at(self.#ident.span));
                }
            }
        }
        // An optional block, marked or unmarked. An absent block is `None` and
        // omitted. The populate marker never applies here, because the source
        // view fills no defaults.
        FieldShape::Nested { optional: true, .. } => {
            quote! {
                if let ::core::option::Option::Some(__child) = &self.#ident {
                    if !__child.span.is_detached() {
                        __items.push(::confval::format::Field::detached_block(
                            #name,
                            ::confval::format::ToFields::to_source_fields(&__child.value),
                        ).at(__child.span));
                    }
                }
            }
        }
        // Each element of a nested list keeps its own span. An empty list is
        // omitted, and a hand-built element with a detached span is skipped.
        FieldShape::NestedList { .. } => {
            quote! {
                for __child in &self.#ident {
                    if !__child.span.is_detached() {
                        __items.push(::confval::format::Field::detached_block(
                            #name,
                            ::confval::format::ToFields::to_source_fields(&__child.value),
                        ).at(__child.span));
                    }
                }
            }
        }
        // A map holds no wrapper span, so its entries keep the only locations
        // it has. An entry whose value span is detached was never written by a
        // source and is dropped, and a map with nothing left is omitted. A
        // source-written empty map is therefore indistinguishable from an
        // absent one, the same limitation the bare string list documents.
        FieldShape::Map => {
            quote! {
                {
                    let __entries: ::std::vec::Vec<::confval::format::Field> = self
                        .#ident
                        .iter()
                        .filter(|(_, __value)| !__value.span.is_detached())
                        .map(|(__key, __value)| {
                            ::confval::format::Field::detached_value(
                                __key,
                                ::confval::format::Value::spanned(
                                    __value.span,
                                    ::confval::format::ValueKind::Scalar(
                                        ::confval::format::Scalar::String(__value.value.clone()),
                                    ),
                                ),
                            )
                        })
                        .collect();
                    if !__entries.is_empty() {
                        __items.push(::confval::format::Field::detached_value(
                            #name,
                            ::confval::format::Value::detached(
                                ::confval::format::ValueKind::Map(
                                    ::confval::format::Fields::detached(__entries),
                                ),
                            ),
                        ));
                    }
                }
            }
        }
    }
}

/// The closure that maps one `Located<String>` element to a spanned string
/// `Value`, preserving each element's location.
fn spanned_string_element() -> TokenStream2 {
    quote! {
        |__element| ::confval::format::Value::spanned(
            __element.span,
            ::confval::format::ValueKind::Scalar(
                ::confval::format::Scalar::String(__element.value.clone()),
            ),
        )
    }
}
