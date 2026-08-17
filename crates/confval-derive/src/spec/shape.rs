//! Working out, from a field's written type, how it should be parsed.
//!
//! Before the code generator can emit a parser for a field, it has to know what
//! kind of thing the field is: a single scalar, a list of strings, a nested
//! sub-struct, and so on. [`classify`] answers that by looking at the field's
//! type and returning a [`FieldShape`]. The generator in the parent module then
//! switches on that shape.

use crate::common::{last_segment, two_generic_args, unwrap_generic};
use syn::{Field, Type, spanned::Spanned};

/// The kind of a spec field, which decides how it is parsed.
///
/// Each variant lists the Rust types it covers. `Located<T>` is confval's
/// "value plus its location in the source file" wrapper, and `S` is another
/// `#[derive(Spec)]` struct.
pub(crate) enum FieldShape {
    /// A single scalar value, such as `Located<String>` or
    /// `Option<Located<i64>>`. `optional` is true when wrapped in `Option`.
    Leaf { leaf: Leaf, optional: bool },
    /// A required list of strings written as `Vec<Located<String>>`. The outer
    /// list is unwrapped, so the parsed field is a plain `Vec`.
    BareStringList,
    /// An optional list of strings written as
    /// `Option<Located<Vec<Located<String>>>>`. Unlike the bare form, this
    /// keeps the outer `Located` so the whole list still carries a location.
    OptionalWrappedStringList,
    /// A single nested sub-struct, `Located<S>` or `Option<Located<S>>`, parsed
    /// by recursing into `S`'s own generated parser. `optional` is true when
    /// wrapped in `Option`. `spec_ty` is the inner type `S`, captured so the
    /// populate walk can emit `<S as Default>::default()` when it fills an
    /// absent marked block. The parser and traversal ignore it. It is boxed
    /// because a `syn::Type` is large and would otherwise bloat every variant.
    Nested { optional: bool, spec_ty: Box<Type> },
    /// A repeated nested sub-struct, `Vec<Located<S>>` (zero or more blocks).
    /// `spec_ty` is the element type `S`, captured so the template walk can
    /// read `S`'s own doc for the commented entry an empty list renders.
    NestedList { spec_ty: Box<Type> },
    /// An open-ended, string-keyed map, `BTreeMap<String, Located<String>>`,
    /// marked `#[confval(map)]`. The value keeps its span, and the key is a
    /// plain `String`. Only the bare form is supported.
    Map,
}

/// The scalar leaf types confval knows how to parse directly.
pub(crate) enum Leaf {
    String,
    Int,
    Float,
    Bool,
    PathBuf,
}

/// Figures out how a field should be parsed from its written type.
///
/// `nested` is the field's `#[confval(nested)]` flag. When set, the field is
/// expected to be a sub-struct (or list of them) and is matched against the
/// nested shapes. `map` is the `#[confval(map)]` flag, mutually exclusive with
/// `nested`, and when set the field must be a `BTreeMap<String,
/// Located<String>>`. When neither is set, the field must be a leaf scalar or a
/// string list. A type that fits none of the supported shapes is a compile
/// error whose message names what was expected, pointing at the field's type.
pub(crate) fn classify(field: &Field, nested: bool, map: bool) -> syn::Result<FieldShape> {
    let ty = &field.ty;
    // First peel off an outer `Option`, if there is one. That wrapper is what
    // makes a field optional. Everything below classifies what is inside it.
    let (optional, inner) = match unwrap_generic(ty, "Option") {
        Some(inner) => (true, inner),
        None => (false, ty),
    };

    // Each attribute flag selects a shape family. With neither flag the field is
    // a plain leaf scalar or string list.
    if map {
        classify_map(ty, inner, optional)
    } else if nested {
        classify_nested(ty, inner, optional)
    } else {
        classify_plain(ty, inner, optional)
    }
}

/// Classifies a `#[confval(map)]` field, a bare `BTreeMap<String,
/// Located<String>>`. Only the bare form ships, so an `Option` wrapper is
/// rejected.
fn classify_map(ty: &Type, inner: &Type, optional: bool) -> syn::Result<FieldShape> {
    if optional {
        return Err(syn::Error::new(
            ty.span(),
            "optional maps are not supported; use a bare \
             BTreeMap<String, Located<String>>",
        ));
    }
    let string_key = two_generic_args(inner, "BTreeMap").is_some_and(|(key, value)| {
        last_segment(key).as_deref() == Some("String") && is_located_string(value)
    });
    if !string_key {
        return Err(syn::Error::new(
            ty.span(),
            "map fields must be BTreeMap<String, Located<String>>",
        ));
    }
    Ok(FieldShape::Map)
}

/// Classifies a `#[confval(nested)]` field, a sub-struct written as either a
/// list of them (`Vec<Located<S>>`) or a single one (`Located<S>` or
/// `Option<Located<S>>`).
fn classify_nested(ty: &Type, inner: &Type, optional: bool) -> syn::Result<FieldShape> {
    if let Some(vec_inner) = unwrap_generic(inner, "Vec") {
        if optional {
            return Err(syn::Error::new(
                ty.span(),
                "nested lists are zero-or-more already; drop the Option",
            ));
        }
        if let Some(spec_ty) = unwrap_generic(vec_inner, "Located") {
            return Ok(FieldShape::NestedList {
                spec_ty: Box::new(spec_ty.clone()),
            });
        }
        return Err(syn::Error::new(
            ty.span(),
            "nested list fields must be Vec<Located<S>>",
        ));
    }
    if let Some(spec_ty) = unwrap_generic(inner, "Located") {
        return Ok(FieldShape::Nested {
            optional,
            spec_ty: Box::new(spec_ty.clone()),
        });
    }
    Err(syn::Error::new(
        ty.span(),
        "nested fields must be Located<S>, Option<Located<S>>, or Vec<Located<S>>",
    ))
}

/// Classifies a plain field, one with neither flag. It is normally wrapped in
/// `Located`, holding either a string list or a single leaf scalar. The one
/// exception is a required string list, a bare `Vec<Located<String>>`.
fn classify_plain(ty: &Type, inner: &Type, optional: bool) -> syn::Result<FieldShape> {
    if let Some(located_inner) = unwrap_generic(inner, "Located") {
        if let Some(vec_inner) = unwrap_generic(located_inner, "Vec") {
            if is_located_string(vec_inner) {
                if optional {
                    return Ok(FieldShape::OptionalWrappedStringList);
                }
                return Err(syn::Error::new(
                    ty.span(),
                    "use Vec<Located<String>> for a required string list",
                ));
            }
            return Err(syn::Error::new(
                ty.span(),
                "list fields must be Vec<Located<String>>",
            ));
        }
        let leaf = leaf_type(located_inner, ty)?;
        return Ok(FieldShape::Leaf { leaf, optional });
    }

    // A required string list is the one shape not wrapped in `Located`. It is
    // written as a bare `Vec<Located<String>>`.
    if let Some(vec_inner) = unwrap_generic(inner, "Vec")
        && !optional
        && is_located_string(vec_inner)
    {
        return Ok(FieldShape::BareStringList);
    }

    Err(syn::Error::new(
        ty.span(),
        "unsupported Spec field type; expected Located<T>, Option<Located<T>>, \
         Vec<Located<String>>, or a #[confval(nested)] structure",
    ))
}

/// The leaf scalar kind named by the type inside a `Located` wrapper, or an
/// error naming the supported scalars. `ty` carries the whole field type, so the
/// error points where the other field-type errors do.
fn leaf_type(located_inner: &Type, ty: &Type) -> syn::Result<Leaf> {
    match last_segment(located_inner).as_deref() {
        Some("String") => Ok(Leaf::String),
        Some("i64") => Ok(Leaf::Int),
        Some("f64") => Ok(Leaf::Float),
        Some("bool") => Ok(Leaf::Bool),
        Some("PathBuf") => Ok(Leaf::PathBuf),
        _ => Err(syn::Error::new(
            ty.span(),
            "unsupported leaf type; expected String, i64, f64, bool, or PathBuf \
             inside Located, or mark the field #[confval(nested)]",
        )),
    }
}

fn is_located_string(ty: &Type) -> bool {
    unwrap_generic(ty, "Located")
        .is_some_and(|element| last_segment(element).as_deref() == Some("String"))
}
