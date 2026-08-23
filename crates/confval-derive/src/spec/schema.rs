//! `#[derive(Spec)]`'s type-level walk: generating `impl ToSchema`.
//!
//! Where the populate walk reads a spec instance and builds a `Fields`, this
//! walk reads only the type and builds a `Schema`. It emits one `SchemaField`
//! per field, read off the same `FieldShape` and `FieldOptions` the parser and
//! the populate walk are built from, so the schema cannot drift from them.
//!
//! Which shape may carry which recording attribute is settled here, not in
//! `options.rs`. A `#[confval(keywords = ...)]` takes a `String` leaf or a
//! string list, and a `#[confval(range = ...)]` takes an `Int` or `Float` leaf.
//! `options.rs` reads the attribute tokens and never classifies the field type,
//! so the rule runs in this module, the only place the shape is known.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::ext::IdentExt;
use syn::{Ident, Path};

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

/// Rejects the misuses of `#[confval(label)]`.
///
/// A block's label is one required string, so the field must be a non-optional
/// `String` leaf and cannot carry a default. A list, a map, a block, or a
/// non-string scalar cannot be a label. An optional leaf names nothing when the
/// block carries no label. A default would build a value the reference pass then
/// reports as undefined.
fn reject_label_misuse(shape: &FieldShape, options: &FieldOptions) -> syn::Result<()> {
    let Some(label) = &options.label else {
        return Ok(());
    };
    match shape {
        FieldShape::Leaf {
            leaf: Leaf::String,
            optional,
            ..
        } => {
            if *optional {
                return Err(syn::Error::new_spanned(
                    label,
                    "#[confval(label)] cannot be optional",
                ));
            }
        }
        // A non-string scalar leaf, such as an integer.
        FieldShape::Leaf { .. } => {
            return Err(syn::Error::new_spanned(
                label,
                "#[confval(label)] requires a String leaf",
            ));
        }
        // A list, a map, or a nested block, matching the constraint rejects.
        _ => {
            return Err(syn::Error::new_spanned(
                label,
                "#[confval(label)] requires a String leaf; \
                 it cannot apply to a list, a map, or a nested block",
            ));
        }
    }
    if options.default.is_some() {
        return Err(syn::Error::new_spanned(
            label,
            "#[confval(label)] cannot take a default",
        ));
    }
    Ok(())
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

/// The `Option<Constraint>` expression a field records, and the one place a
/// (shape, attribute) pair is judged legal.
///
/// The mutual-exclusion check runs first for every shape, so a field carrying
/// two recording attributes reads that mistake rather than a pairing message
/// about one of them. What each shape can then carry differs. A scalar leaf
/// records all three against its leaf type. A string list records `keywords`
/// alone, for the set each element must come from. A map and a nested block
/// record nothing.
fn constraint_tokens(shape: &FieldShape, options: &FieldOptions) -> syn::Result<TokenStream2> {
    let Some(recorded) = one_recording_attribute(options)? else {
        return Ok(quote! { ::core::option::Option::None });
    };
    match shape {
        FieldShape::Leaf { leaf, .. } => leaf_constraint(leaf, recorded),
        FieldShape::BareStringList | FieldShape::OptionalWrappedStringList => match recorded {
            Recorded::Keywords(path) => Ok(keywords_tokens(path)),
            Recorded::Range(path) => Err(refused(path, RANGE_REQUIRES, "a list")),
            Recorded::References(block) => Err(refused(block, REFERENCES_REQUIRES, "a list")),
        },
        FieldShape::Nested { .. } | FieldShape::NestedList { .. } | FieldShape::Map => {
            let where_not = "a map or a nested block";
            match recorded {
                Recorded::Keywords(path) => Err(refused(path, KEYWORDS_REQUIRES, where_not)),
                Recorded::Range(path) => Err(refused(path, RANGE_REQUIRES, where_not)),
                Recorded::References(block) => Err(refused(block, REFERENCES_REQUIRES, where_not)),
            }
        }
    }
}

const KEYWORDS_REQUIRES: &str =
    "#[confval(keywords = ...)] requires a String leaf or a string list";
const RANGE_REQUIRES: &str = "#[confval(range = ...)] requires an Int or Float leaf";
const REFERENCES_REQUIRES: &str = "#[confval(references = ...)] requires a String leaf";

/// The one recording attribute a field declares.
enum Recorded<'a> {
    Keywords(&'a Path),
    Range(&'a Path),
    References(&'a Ident),
}

/// The recording attribute a field declares, or `None` when it declares none.
///
/// Two of them on one field is an error wherever the field sits, because no
/// shape carries two constraints.
fn one_recording_attribute(options: &FieldOptions) -> syn::Result<Option<Recorded<'_>>> {
    let too_many = "a field takes at most one of #[confval(keywords = ...)], \
                    #[confval(range = ...)], or #[confval(references = ...)]";
    match (&options.keywords, &options.range, &options.references) {
        (Some(_), Some(range), _) => Err(syn::Error::new_spanned(range, too_many)),
        (Some(_), _, Some(references)) => Err(syn::Error::new_spanned(references, too_many)),
        (_, Some(_), Some(references)) => Err(syn::Error::new_spanned(references, too_many)),
        (Some(path), None, None) => Ok(Some(Recorded::Keywords(path))),
        (None, Some(path), None) => Ok(Some(Recorded::Range(path))),
        (None, None, Some(block)) => Ok(Some(Recorded::References(block))),
        (None, None, None) => Ok(None),
    }
}

/// The constraint a scalar leaf records, paired against its leaf type.
fn leaf_constraint(leaf: &Leaf, recorded: Recorded<'_>) -> syn::Result<TokenStream2> {
    match recorded {
        Recorded::Keywords(path) => {
            if !matches!(leaf, Leaf::String) {
                return Err(syn::Error::new_spanned(path, KEYWORDS_REQUIRES));
            }
            Ok(keywords_tokens(path))
        }
        Recorded::Range(path) => {
            if !matches!(leaf, Leaf::Int | Leaf::Float) {
                return Err(syn::Error::new_spanned(path, RANGE_REQUIRES));
            }
            // A float bound renders through `{:?}`, the form the default text
            // uses, so a whole-number bound keeps its `.0` and hover on a
            // float field reads float text.
            let (min, max) = match leaf {
                Leaf::Float => (
                    quote! { ::std::format!("{:?}", #path.min) },
                    quote! { ::std::format!("{:?}", #path.max) },
                ),
                _ => (
                    quote! { ::std::string::ToString::to_string(&#path.min) },
                    quote! { ::std::string::ToString::to_string(&#path.max) },
                ),
            };
            Ok(quote! {
                ::core::option::Option::Some(
                    ::confval::schema::Constraint::range(
                        #min,
                        #max,
                        #path.units,
                        #path.help,
                    ),
                )
            })
        }
        Recorded::References(block) => {
            if !matches!(leaf, Leaf::String) {
                return Err(syn::Error::new_spanned(block, REFERENCES_REQUIRES));
            }
            let block = block.unraw().to_string();
            Ok(quote! {
                ::core::option::Option::Some(
                    ::confval::schema::Constraint::references(#block),
                )
            })
        }
    }
}

/// The `Constraint::keywords` expression reading a `keyword_enum!` type's set.
fn keywords_tokens(path: &Path) -> TokenStream2 {
    quote! {
        ::core::option::Option::Some(
            ::confval::schema::Constraint::keywords(&#path::KEYWORDS),
        )
    }
}

/// A refusal naming what the attribute needs and the shape it cannot sit on.
fn refused<T: quote::ToTokens>(at: &T, requires: &str, where_not: &str) -> syn::Error {
    syn::Error::new_spanned(at, format!("{requires}; it cannot apply to {where_not}"))
}

/// The `Option<String>` expression for a doc comment, built for `Schema::doc` and
/// `SchemaField::doc`.
fn option_string(text: &Option<String>) -> TokenStream2 {
    match text {
        Some(text) => quote! { ::core::option::Option::Some(#text.to_string()) },
        None => quote! { ::core::option::Option::None },
    }
}
