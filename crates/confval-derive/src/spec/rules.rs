//! Legality checks for `#[derive(Spec)]` field attributes.
//!
//! Each check returns `Ok(())` when its attribute is absent, so a field without
//! the attribute is never rejected. The checks run once per field in
//! [`super::expand`]'s classification loop, before the parsing fragments are
//! emitted, so a misplaced attribute is reported at compile time.

use super::options::FieldOptions;
use super::shape::FieldShape;
use syn::Field;
use syn::spanned::Spanned;

/// Rejects `#[confval(default ...)]` on field shapes that would silently ignore
/// it. Only leaf fields honor a default value. A string list accepts a bare
/// `#[confval(default)]` (meaning "empty") but not `default = <expr>`. Every
/// other shape would ignore the default, so flag it at compile time rather than
/// surprise the author at runtime.
pub(crate) fn reject_unsupported_default(
    field: &Field,
    shape: &FieldShape,
    options: &FieldOptions,
) -> syn::Result<()> {
    let Some(default) = &options.default else {
        return Ok(());
    };
    let supported = match shape {
        FieldShape::Leaf { .. } => true,
        // A string list's only meaningful default is the empty list, written as
        // a bare `#[confval(default)]`. An explicit value cannot be honored.
        FieldShape::BareStringList => default.is_none(),
        // A nested block, optional or not, accepts a bare `#[confval(default)]`.
        // On a non-optional block the parser fills an absent block with the
        // type's `Default`. On an optional block the bare default is the
        // populate marker `ToFields` reads to fill an absent block, and parsing
        // still leaves it `None`. Neither has a sensible `default = expr` for a
        // whole sub-struct.
        FieldShape::Nested {
            optional: false, ..
        }
        | FieldShape::Nested { optional: true, .. } => default.is_none(),
        // A map's only meaningful default is the empty map, a bare
        // `#[confval(default)]`. A `default = expr` for a whole map is refused.
        FieldShape::Map => default.is_none(),
        FieldShape::NestedList { .. } | FieldShape::OptionalWrappedStringList => false,
    };
    if supported {
        return Ok(());
    }
    let span = default
        .as_ref()
        .map(Spanned::span)
        .unwrap_or_else(|| field.span());
    Err(syn::Error::new(
        span,
        "#[confval(default)] is not supported here; a leaf field takes \
         #[confval(default)] or #[confval(default = expr)], while a string list, \
         a map, or a nested block accepts only a bare #[confval(default)]",
    ))
}

/// Rejects a field-level `#[confval(derive_default = expr)]` that cannot be
/// honored.
///
/// The value fills only the generated `Default` impl, so it needs the
/// struct-level flag. It cannot stand beside `#[confval(default)]`, which
/// already supplies the same value and also makes the field optional. It cannot
/// stand beside `#[confval(non_empty)]` or `#[confval(unique)]`, because the
/// macro cannot prove that an arbitrary expression passes either check. It has
/// no meaning on an optional field, whose generated value is already `None`.
pub(crate) fn reject_derive_default(
    shape: &FieldShape,
    options: &FieldOptions,
    struct_default: bool,
) -> syn::Result<()> {
    let Some(expr) = &options.derive_default_value else {
        return Ok(());
    };
    if !struct_default {
        return Err(syn::Error::new_spanned(
            expr,
            "#[confval(derive_default = expr)] needs #[confval(derive_default)] \
             on the struct",
        ));
    }
    if options.default.is_some() {
        return Err(syn::Error::new_spanned(
            expr,
            "#[confval(derive_default)] cannot be combined with #[confval(default)]. \
             `default` already supplies the value for an absent field and the \
             generated `Default` impl.",
        ));
    }
    if options.non_empty.is_some() {
        return Err(syn::Error::new_spanned(
            expr,
            "#[confval(derive_default)] cannot be combined with #[confval(non_empty)]. \
             A generated string or list can be empty.",
        ));
    }
    if options.unique.is_some() {
        return Err(syn::Error::new_spanned(
            expr,
            "#[confval(derive_default)] cannot be combined with #[confval(unique)]. \
             A generated list can repeat a value.",
        ));
    }
    let optional = match shape {
        FieldShape::Leaf { optional, .. } | FieldShape::Nested { optional, .. } => *optional,
        FieldShape::OptionalWrappedStringList => true,
        FieldShape::BareStringList | FieldShape::Map | FieldShape::NestedList { .. } => false,
    };
    if optional {
        return Err(syn::Error::new_spanned(
            expr,
            "#[confval(derive_default)] has no meaning on an optional field. \
             An absent field is already `None` in the generated `Default` impl.",
        ));
    }
    Ok(())
}
