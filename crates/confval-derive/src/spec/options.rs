//! Reading the per-field `#[confval(...)]` options for `#[derive(Spec)]`.
//!
//! A spec field can be annotated with `#[confval(nested)]`,
//! `#[confval(default)]` / `#[confval(default = expr)]`, and
//! `#[confval(doc = "...")]`. This module turns those attributes, plus a
//! harvested `///` doc comment, into a plain [`FieldOptions`] struct the rest of
//! the derive reads.

use syn::{DeriveInput, Expr, Field};

/// The struct-level `#[confval(...)]` options for `#[derive(Spec)]`.
pub(crate) struct StructOptions {
    /// `true` if the struct was marked `#[confval(derive_default)]`, which asks
    /// the derive to generate the `Default` impl from the fields' attribute
    /// defaults.
    pub(crate) derive_default: bool,
}

/// Reads a struct's `#[confval(...)]` attributes into [`StructOptions`].
///
/// Recognizes `derive_default`. An unknown key is a compile error, so a typo
/// like `#[confval(derive_defalt)]` is caught rather than ignored. The Config
/// derive's own struct keys are unknown here, so a type that derives both `Spec`
/// and `Config` is rejected. Separate spec and config types are the expected
/// shape.
pub(crate) fn parse_struct_options(input: &DeriveInput) -> syn::Result<StructOptions> {
    let mut options = StructOptions {
        derive_default: false,
    };
    for attr in &input.attrs {
        if !attr.path().is_ident("confval") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("derive_default") {
                options.derive_default = true;
                Ok(())
            } else {
                Err(meta.error("unknown confval attribute; expected `derive_default`"))
            }
        })?;
    }
    Ok(options)
}

/// What a field's `#[confval(...)]` attributes asked for.
pub(crate) struct FieldOptions {
    /// `true` if the field was marked `#[confval(nested)]`, i.e. it is a
    /// sub-struct rather than a scalar.
    pub(crate) nested: bool,
    /// Whether a `default` was requested, and with what value. The two `Option`
    /// layers mean different things:
    ///
    /// - `None`              no `default` attribute.
    /// - `Some(None)`        `#[confval(default)]`, use the type's `Default`.
    /// - `Some(Some(expr))`  `#[confval(default = expr)]`, use `expr`.
    pub(crate) default: Option<Option<Expr>>,
    /// The doc comment `to_template` renders above the field, or `None`. Comes
    /// from `#[confval(doc = "...")]` if present, otherwise the field's `///`
    /// doc comments joined into one string.
    pub(crate) doc: Option<String>,
}

/// Reads a field's `#[confval(...)]` attributes into [`FieldOptions`].
///
/// Walks every `#[confval(...)]` attribute on the field and records the keys it
/// recognizes. An unrecognized key is a compile error, so a typo like
/// `#[confval(nestd)]` is caught rather than silently ignored.
pub(crate) fn parse_options(field: &Field) -> syn::Result<FieldOptions> {
    let mut options = FieldOptions {
        nested: false,
        default: None,
        doc: None,
    };
    let mut confval_doc = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("confval") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("nested") {
                options.nested = true;
                Ok(())
            } else if meta.path.is_ident("default") {
                if meta.input.peek(syn::Token![=]) {
                    let expr: Expr = meta.value()?.parse()?;
                    options.default = Some(Some(expr));
                } else {
                    options.default = Some(None);
                }
                Ok(())
            } else if meta.path.is_ident("doc") {
                let text: syn::LitStr = meta.value()?.parse()?;
                confval_doc = Some(text.value());
                Ok(())
            } else {
                Err(meta.error("unknown confval attribute; expected `nested`, `default`, or `doc`"))
            }
        })?;
    }
    // A `#[confval(doc = "...")]` overrides the harvested `///` comment.
    options.doc = confval_doc.or_else(|| harvest_doc_comment(&field.attrs));
    Ok(options)
}

/// Joins a field's `///` doc comments into one string, one line per `#[doc]`
/// attribute, trimming the single leading space `///` inserts on each line.
/// Returns `None` when the field has no doc comment.
fn harvest_doc_comment(attrs: &[syn::Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(name_value) => match &name_value.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => {
                    let raw = text.value();
                    Some(raw.strip_prefix(' ').unwrap_or(&raw).to_string())
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}
