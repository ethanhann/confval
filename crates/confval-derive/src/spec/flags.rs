//! The legality rules for `#[derive(Spec)]`'s flag attributes, `label`,
//! `non_empty`, and `unique`, and the parser for the `help = "..."` list the
//! last two take. Each flag has its own set of shapes it applies
//! to and its own rule about `default`, so each has its own function.
//! The value constraints have their rules in [`recorded`](super::recorded).
//!
//! The schema walk calls every function here for every field, and each one
//! returns early when its flag is absent, so a misplaced flag is a compile
//! error before the validation walk runs.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};

/// Rejects the misuses of `#[confval(label)]`.
///
/// A block's label is one required string, so the field must be a non-optional
/// `String` leaf and cannot have a default. A list, a map, a block, or a
/// non-string scalar cannot be a label. An optional leaf names nothing when the
/// block has no label. A default would build a value the reference pass then
/// reports as undefined.
pub(crate) fn reject_label_misuse(shape: &FieldShape, options: &FieldOptions) -> syn::Result<()> {
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

/// Rejects the misuses of `#[confval(non_empty)]`.
///
/// `non_empty` is valid on a `String` leaf and on a string list. It rejects
/// `Int`, `Float`, `Bool`, `Path`, `Block`, and `Map`. It combines with the
/// value constraints valid on a string, which are `keywords`, `length`,
/// `format`, and `references`. It does not combine with `range`, because
/// `range` requires an `Int` or `Float` leaf. A field with both `label` and
/// `non_empty` is rejected, because the reference pass owns the empty-label
/// report. A field with both `default` and `non_empty` is rejected.
/// The default for a `String` is the empty string and for a list is the
/// empty list. Either one would fail the check.
pub(crate) fn reject_non_empty_misuse(
    shape: &FieldShape,
    options: &FieldOptions,
) -> syn::Result<()> {
    let Some(non_empty) = &options.non_empty else {
        return Ok(());
    };
    match shape {
        FieldShape::Leaf {
            leaf: Leaf::String, ..
        }
        | FieldShape::BareStringList
        | FieldShape::OptionalWrappedStringList => {}
        // A non-string scalar leaf, such as an integer.
        FieldShape::Leaf { .. } => {
            return Err(syn::Error::new_spanned(
                non_empty,
                "#[confval(non_empty)] requires a String leaf or a string list",
            ));
        }
        // A map or a nested block.
        _ => {
            return Err(syn::Error::new_spanned(
                non_empty,
                "#[confval(non_empty)] requires a String leaf or a string list; \
                 it cannot apply to a map or a nested block",
            ));
        }
    }
    if options.label.is_some() {
        return Err(syn::Error::new_spanned(
            non_empty,
            "#[confval(non_empty)] cannot be combined with #[confval(label)]; \
             check_references reports an empty label, so run that pass beside validate_all",
        ));
    }
    if options.default.is_some() {
        return Err(syn::Error::new_spanned(
            non_empty,
            "#[confval(non_empty)] cannot be combined with #[confval(default)]; \
             the default for a String is the empty string, which fails the check",
        ));
    }
    Ok(())
}

/// Rejects the misuses of `#[confval(unique)]`.
///
/// `unique` is valid on the two string list shapes alone. A scalar leaf has
/// one value, so nothing can repeat, and a map's keys are unique already. It
/// combines with `keywords`, `format`, `non_empty`, and `default`, because
/// the default list is empty and so unique.
pub(crate) fn reject_unique_misuse(shape: &FieldShape, options: &FieldOptions) -> syn::Result<()> {
    let Some(unique) = &options.unique else {
        return Ok(());
    };
    match shape {
        FieldShape::BareStringList | FieldShape::OptionalWrappedStringList => Ok(()),
        // A scalar leaf of any type, which holds one value.
        FieldShape::Leaf { .. } => Err(syn::Error::new_spanned(
            unique,
            "#[confval(unique)] requires a string list",
        )),
        // A map or a nested block.
        _ => Err(syn::Error::new_spanned(
            unique,
            "#[confval(unique)] requires a string list; \
             it cannot apply to a map or a nested block",
        )),
    }
}

/// Stores a flag's own path and, when a parenthesized list follows, its
/// `help = "..."` line. Rejects a second flag, an unknown key, a non-string
/// help, and a second help. `label` takes no list and uses `set_flag` in
/// `options.rs` instead.
pub(crate) fn set_flag_with_help(
    slot: &mut Option<syn::Path>,
    help: &mut Option<syn::LitStr>,
    meta: &syn::meta::ParseNestedMeta<'_>,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate confval attribute `{key}`")));
    }
    *slot = Some(meta.path.clone());
    if !meta.input.peek(syn::token::Paren) {
        return Ok(());
    }
    meta.parse_nested_meta(|inner| {
        if !inner.path.is_ident("help") {
            return Err(inner.error(format!(
                "unknown key in `{key}(...)`; expected `help = \"...\"`"
            )));
        }
        if help.is_some() {
            return Err(inner.error(format!("duplicate `help` in `{key}(...)`")));
        }
        let text: syn::LitStr = inner.value()?.parse()?;
        *help = Some(text);
        Ok(())
    })
}
