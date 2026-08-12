//! `#[derive(Spec)]`'s type-level walk: generating `impl ToSchema`.
//!
//! Where the populate walk reads a spec instance and builds a `Fields`, this
//! walk reads only the type and builds a `Schema`. It emits one `SchemaField`
//! per field, read off the same `FieldShape` and `FieldOptions` the parser and
//! the populate walk are built from, so the schema cannot drift from them.
//!
//! The leaf-type pairing check for the two recording attributes runs here, not
//! in `options.rs`. A `#[confval(keywords = ...)]` requires a `String` leaf and
//! a `#[confval(range = ...)]` requires an `Int` or `Float` leaf. `options.rs`
//! reads the attribute tokens and never classifies the field type, so the pairing
//! rule runs in this module, the only place the `Leaf` is known.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;
use syn::ext::IdentExt;

/// Assembles the `impl ToSchema` for one struct from its per-field fragments and
/// the struct's own doc comment. The generated `schema()` builds the level
/// through `Schema::new`, because `Schema` is `#[non_exhaustive]` and a struct
/// literal is a compile error in the caller's crate.
pub(crate) fn to_schema_impl(
    name: &Ident,
    doc: &Option<String>,
    fields: &[TokenStream2],
) -> TokenStream2 {
    let doc = option_string(doc);
    quote! {
        impl ::confval::schema::ToSchema for #name {
            fn schema() -> ::confval::schema::Schema {
                ::confval::schema::Schema::new(
                    #doc,
                    ::std::vec::Vec::from([
                        #(#fields),*
                    ]),
                )
            }
        }
    }
}

/// The `SchemaField::new(...)` construction for one field, or a compile error
/// when a recording attribute is paired with the wrong leaf or a non-scalar
/// shape.
///
/// The constructor folds the default into `required`, so this passes the
/// field's `structurally_required` flag and lets `SchemaField::new` compute
/// `required` as `structurally_required && !has_default`.
pub(crate) fn field_schema(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> syn::Result<TokenStream2> {
    let name = ident.unraw().to_string();
    let doc = option_string(&options.doc);
    let has_default = options.default.is_some();
    let structurally_required = structurally_required(shape);
    let ty = schema_type(shape, options)?;
    Ok(quote! {
        ::confval::schema::SchemaField::new(
            #name.to_string(),
            #doc,
            #structurally_required,
            #has_default,
            #ty,
        )
    })
}

/// Whether an absent field is a parse error before the default is folded in.
/// This is the "structurally required" column of the mapping table.
fn structurally_required(shape: &FieldShape) -> bool {
    match shape {
        FieldShape::Leaf { optional, .. } => !*optional,
        FieldShape::BareStringList => true,
        FieldShape::OptionalWrappedStringList => false,
        FieldShape::Nested { optional, .. } => !*optional,
        FieldShape::NestedList { .. } => false,
        FieldShape::Map => true,
    }
}

/// The `SchemaType` expression for a field's shape, running the constraint
/// pairing check on the way.
fn schema_type(shape: &FieldShape, options: &FieldOptions) -> syn::Result<TokenStream2> {
    match shape {
        FieldShape::Leaf { leaf, .. } => {
            let scalar = scalar_type(leaf);
            let constraint = constraint_tokens(leaf, options)?;
            Ok(quote! {
                ::confval::schema::SchemaType::Scalar {
                    leaf: #scalar,
                    constraint: #constraint,
                }
            })
        }
        FieldShape::BareStringList | FieldShape::OptionalWrappedStringList => {
            reject_constraint_on_non_scalar(options)?;
            Ok(quote! { ::confval::schema::SchemaType::StringList })
        }
        FieldShape::Nested { spec_ty, .. } => {
            reject_constraint_on_non_scalar(options)?;
            let spec_ty = &**spec_ty;
            Ok(block_type(spec_ty, false))
        }
        FieldShape::NestedList { spec_ty } => {
            reject_constraint_on_non_scalar(options)?;
            let spec_ty = &**spec_ty;
            Ok(block_type(spec_ty, true))
        }
        FieldShape::Map => {
            reject_constraint_on_non_scalar(options)?;
            Ok(quote! { ::confval::schema::SchemaType::StringMap })
        }
    }
}

/// A `Block` node that recurses into the child's own `schema()` through the
/// `ToSchema` bound, so one call at the root builds the whole tree. A handwritten
/// child nested here must implement `ToSchema`, the way it already implements
/// `ToFields`.
fn block_type(spec_ty: &syn::Type, repeated: bool) -> TokenStream2 {
    quote! {
        ::confval::schema::SchemaType::Block {
            schema: ::std::boxed::Box::new(
                <#spec_ty as ::confval::schema::ToSchema>::schema(),
            ),
            repeated: #repeated,
        }
    }
}

fn scalar_type(leaf: &Leaf) -> TokenStream2 {
    match leaf {
        Leaf::String => quote! { ::confval::schema::ScalarType::String },
        Leaf::Int => quote! { ::confval::schema::ScalarType::Int },
        Leaf::Float => quote! { ::confval::schema::ScalarType::Float },
        Leaf::Bool => quote! { ::confval::schema::ScalarType::Bool },
        Leaf::PathBuf => quote! { ::confval::schema::ScalarType::Path },
    }
}

/// The `Option<Constraint>` expression for a scalar leaf, and the site of the
/// pairing check. `keywords` requires a `String` leaf, `range` requires an `Int`
/// or `Float` leaf, and the two cannot share a field, because one leaf cannot be
/// both.
fn constraint_tokens(leaf: &Leaf, options: &FieldOptions) -> syn::Result<TokenStream2> {
    match (&options.keywords, &options.range) {
        (Some(_), Some(range)) => Err(syn::Error::new_spanned(
            range,
            "a field takes either #[confval(keywords = ...)] or #[confval(range = ...)], \
             not both; keywords requires a String leaf and range requires an Int or Float leaf",
        )),
        (Some(path), None) => {
            if !matches!(leaf, Leaf::String) {
                return Err(syn::Error::new_spanned(
                    path,
                    "#[confval(keywords = ...)] requires a String leaf",
                ));
            }
            Ok(quote! {
                ::core::option::Option::Some(
                    ::confval::schema::Constraint::Keywords(&#path::KEYWORDS),
                )
            })
        }
        (None, Some(path)) => {
            if !matches!(leaf, Leaf::Int | Leaf::Float) {
                return Err(syn::Error::new_spanned(
                    path,
                    "#[confval(range = ...)] requires an Int or Float leaf",
                ));
            }
            Ok(quote! {
                ::core::option::Option::Some(
                    ::confval::schema::Constraint::Range {
                        min: ::std::string::ToString::to_string(&#path.min),
                        max: ::std::string::ToString::to_string(&#path.max),
                        units: #path.units,
                        help: #path.help,
                    },
                )
            })
        }
        (None, None) => Ok(quote! { ::core::option::Option::None }),
    }
}

/// Rejects a recording attribute on a shape that is not a scalar leaf. A list, a
/// map, or a nested block has no closed set or numeric bound to record.
fn reject_constraint_on_non_scalar(options: &FieldOptions) -> syn::Result<()> {
    if let Some(path) = &options.keywords {
        return Err(syn::Error::new_spanned(
            path,
            "#[confval(keywords = ...)] requires a String leaf; \
             it cannot apply to a list, a map, or a nested block",
        ));
    }
    if let Some(path) = &options.range {
        return Err(syn::Error::new_spanned(
            path,
            "#[confval(range = ...)] requires an Int or Float leaf; \
             it cannot apply to a list, a map, or a nested block",
        ));
    }
    Ok(())
}

/// The `Option<String>` expression for a doc comment, built for `Schema::doc` and
/// `SchemaField::doc`.
fn option_string(text: &Option<String>) -> TokenStream2 {
    match text {
        Some(text) => quote! { ::core::option::Option::Some(#text.to_string()) },
        None => quote! { ::core::option::Option::None },
    }
}
