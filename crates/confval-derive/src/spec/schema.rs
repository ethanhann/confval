//! `#[derive(Spec)]`'s type-level walk: generating `impl ToSchema`.
//!
//! Where the populate walk reads a spec instance and builds a `Fields`, this
//! walk reads only the type and builds a `Schema`. It emits one `SchemaField`
//! per field, read off the same `FieldShape` and `FieldOptions` the parser and
//! the populate walk are built from, so the schema cannot drift from them.
//!
//! Which shape may carry which attribute is settled in
//! [`recorded`](super::recorded), which this walk calls for every field, so a
//! misplaced attribute is a compile error before the validation walk runs.

use super::options::FieldOptions;
use super::recorded::{constraint_tokens, reject_label_misuse, reject_non_empty_misuse};
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
    let ty = schema_type(shape, options)?;
    reject_label_misuse(shape, options)?;
    let mut field = quote! {
        ::confval::schema::SchemaField::new(#name.to_string(), #doc, #ty)
    };
    if structurally_required(shape) {
        field = quote! { #field.required() };
    }
    if options.default.is_some() {
        field = quote! { #field.with_default() };
    }
    if let (FieldShape::Leaf { leaf, .. }, Some(expr)) = (shape, options.default_value()) {
        let rendered = default_text(leaf, &expr);
        field = quote! { #field.with_default_text(#rendered) };
    }
    if options.non_empty.is_some() {
        reject_non_empty_misuse(shape, options)?;
        field = quote! { #field.with_non_empty() };
    }
    if options.label.is_some() {
        Ok(quote! { #field.as_label() })
    } else {
        Ok(field)
    }
}

/// The rendered default text for a scalar leaf, evaluated inside the generated
/// `schema()`. The typed binding pins the expression to the leaf's Rust type,
/// so a bare default renders that type's own default. A float renders through
/// `{:?}`, so a whole number keeps its `.0`, the form the emitters write. A
/// path renders through its lossy string form, the form the populate walk
/// reads.
fn default_text(leaf: &Leaf, expr: &TokenStream2) -> TokenStream2 {
    match leaf {
        Leaf::String => quote! {
            { let value: ::std::string::String = #expr; value }
        },
        Leaf::Int => quote! {
            ::std::string::ToString::to_string(&{ let value: i64 = #expr; value })
        },
        Leaf::Float => quote! {
            ::std::format!("{:?}", { let value: f64 = #expr; value })
        },
        Leaf::Bool => quote! {
            ::std::string::ToString::to_string(&{ let value: bool = #expr; value })
        },
        Leaf::PathBuf => quote! {
            {
                let value: ::std::path::PathBuf = #expr;
                value.to_string_lossy().into_owned()
            }
        },
    }
}

/// Rejects a nested field whose type is the spec being derived. The generated
/// `schema()` builds the whole tree eagerly, so a spec that nests itself
/// would recurse without end at the first `schema()` call.
pub(crate) fn reject_self_nesting(spec: &Ident, shape: &FieldShape) -> syn::Result<()> {
    let spec_ty = match shape {
        FieldShape::Nested { spec_ty, .. } | FieldShape::NestedList { spec_ty } => &**spec_ty,
        _ => return Ok(()),
    };
    if let syn::Type::Path(path) = spec_ty
        && path.qself.is_none()
        && path.path.segments.len() == 1
        && path.path.segments[0].ident == *spec
    {
        return Err(syn::Error::new_spanned(
            spec_ty,
            "#[confval(nested)] cannot nest a spec inside itself; \
             a configuration schema is a finite tree",
        ));
    }
    Ok(())
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
    let constraint = constraint_tokens(shape, options)?;
    match shape {
        FieldShape::Leaf { leaf, .. } => {
            let scalar = scalar_type(leaf);
            Ok(quote! {
                ::confval::schema::SchemaType::scalar(#scalar, #constraint)
            })
        }
        FieldShape::BareStringList | FieldShape::OptionalWrappedStringList => Ok(quote! {
            ::confval::schema::SchemaType::string_list(#constraint)
        }),
        FieldShape::Nested { spec_ty, .. } => Ok(block_type(spec_ty, false)),
        FieldShape::NestedList { spec_ty } => Ok(block_type(spec_ty, true)),
        FieldShape::Map => Ok(quote! { ::confval::schema::SchemaType::string_map() }),
    }
}

/// A `Block` node that recurses into the child's own `schema()` through the
/// `ToSchema` bound, so one call at the root builds the whole tree. A handwritten
/// child nested here must implement `ToSchema`, the way it already implements
/// `ToFields`.
fn block_type(spec_ty: &syn::Type, repeated: bool) -> TokenStream2 {
    quote! {
        ::confval::schema::SchemaType::block(
            <#spec_ty as ::confval::schema::ToSchema>::schema(),
            #repeated,
        )
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

/// The `Option<String>` expression for a doc comment, built for `Schema::doc` and
/// `SchemaField::doc`.
fn option_string(text: &Option<String>) -> TokenStream2 {
    match text {
        Some(text) => quote! { ::core::option::Option::Some(#text.to_string()) },
        None => quote! { ::core::option::Option::None },
    }
}
