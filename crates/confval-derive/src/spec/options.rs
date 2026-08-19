//! Reading the per-field `#[confval(...)]` options for `#[derive(Spec)]`.
//!
//! A spec field can be annotated with `#[confval(nested)]`,
//! `#[confval(default)]` / `#[confval(default = expr)]`, and
//! `#[confval(doc = "...")]`. This module turns those attributes, plus a
//! harvested `///` doc comment, into a plain [`FieldOptions`] struct the rest of
//! the derive reads.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, Expr, Field, Ident, Path};

/// The struct-level `#[confval(...)]` options for `#[derive(Spec)]`.
pub(crate) struct StructOptions {
    /// `true` if the struct was marked `#[confval(derive_default)]`, which asks
    /// the derive to generate the `Default` impl from the fields' attribute
    /// defaults.
    pub(crate) derive_default: bool,
    /// The doc comment `spec_doc` returns for this type, or `None`. Comes from
    /// a struct-level `#[confval(doc = "...")]` if present, otherwise the
    /// struct's `///` doc comments joined into one string. A parent's template
    /// walk falls back to it for a block whose embedding field has no doc.
    pub(crate) doc: Option<String>,
}

/// Reads a struct's `#[confval(...)]` attributes into [`StructOptions`].
///
/// Recognizes `derive_default` and `doc`. An unknown key is a compile error, so
/// a typo like `#[confval(derive_defalt)]` is caught rather than ignored. The
/// Config derive's own struct keys are unknown here, so a type that derives both
/// `Spec` and `Config` is rejected. Separate spec and config types are the
/// expected shape.
pub(crate) fn parse_struct_options(input: &DeriveInput) -> syn::Result<StructOptions> {
    let mut options = StructOptions {
        derive_default: false,
        doc: None,
    };
    let mut confval_doc = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("confval") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("derive_default") {
                if options.derive_default {
                    return Err(meta.error("duplicate confval attribute `derive_default`"));
                }
                options.derive_default = true;
                Ok(())
            } else if meta.path.is_ident("doc") {
                if confval_doc.is_some() {
                    return Err(meta.error("duplicate confval attribute `doc`"));
                }
                let text: syn::LitStr = meta.value()?.parse()?;
                confval_doc = Some(text.value());
                Ok(())
            } else {
                Err(meta.error("unknown confval attribute; expected `derive_default` or `doc`"))
            }
        })?;
    }
    // A `#[confval(doc = "...")]` overrides the harvested `///` comment, the
    // same precedence a field has.
    options.doc = confval_doc.or_else(|| harvest_doc_comment(&input.attrs));
    Ok(options)
}

impl FieldOptions {
    /// The value expression a requested default resolves to: the given `expr`
    /// for `#[confval(default = expr)]`, the type's `Default` for a bare
    /// `#[confval(default)]`, and `None` when no default was requested.
    ///
    /// The parser's absent-field fill and the `derive_default` impl both read
    /// this one mapping, so the two cannot disagree on what a default means.
    pub(crate) fn default_value(&self) -> Option<TokenStream2> {
        self.default.as_ref().map(|default| match default {
            Some(expr) => quote! { #expr },
            None => quote! { ::core::default::Default::default() },
        })
    }
}

/// What a field's `#[confval(...)]` attributes asked for.
pub(crate) struct FieldOptions {
    /// `true` if the field was marked `#[confval(nested)]`, i.e. it is a
    /// sub-struct rather than a scalar.
    pub(crate) nested: bool,
    /// `true` if the field was marked `#[confval(map)]`, i.e. it is an
    /// open-ended, string-keyed map. Mutually exclusive with `nested`.
    pub(crate) map: bool,
    /// Whether a `default` was requested, and with what value. The two `Option`
    /// layers mean different things:
    ///
    /// - `None`              no `default` attribute.
    /// - `Some(None)`        `#[confval(default)]`, use the type's `Default`.
    /// - `Some(Some(expr))`  `#[confval(default = expr)]`, use `expr`.
    pub(crate) default: Option<Option<Expr>>,
    /// The path a `#[confval(keywords = PATH)]` names, or `None`. It records the
    /// association from a `String` leaf to a `keyword_enum!` type, whose
    /// `KEYWORDS` const the schema walk reads. The leaf-type pairing is not
    /// checked here. It is checked in `spec/schema.rs`, where the classified
    /// shape is available.
    pub(crate) keywords: Option<Path>,
    /// The path a `#[confval(range = PATH)]` names, or `None`. It records the
    /// association from an `Int` or `Float` leaf to a `RangeConstraint` value,
    /// whose bounds the schema walk renders. The leaf-type pairing is checked in
    /// `spec/schema.rs`.
    pub(crate) range: Option<Path>,
    /// `Some` if the field was marked `#[confval(label)]`, i.e. its value is the
    /// enclosing block's label. HCL and KDL carry the label in the block syntax,
    /// and the other formats carry it as this field. The `String` leaf pairing is
    /// checked in `spec/schema.rs`.
    pub(crate) label: Option<syn::Path>,
    /// The block field name a `#[confval(references = <block>)]` names, or `None`.
    /// The value references the labels of that block, resolved outward to the
    /// nearest enclosing scope that declares it. Unlike `keywords` and `range`,
    /// it is a bare config field name, not a Rust path, stored and emitted as a
    /// string.
    pub(crate) references: Option<Ident>,
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
        map: false,
        default: None,
        keywords: None,
        range: None,
        label: None,
        references: None,
        doc: None,
    };
    let mut confval_doc = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("confval") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("nested") {
                if options.nested {
                    return Err(meta.error("duplicate confval attribute `nested`"));
                }
                options.nested = true;
                Ok(())
            } else if meta.path.is_ident("map") {
                if options.map {
                    return Err(meta.error("duplicate confval attribute `map`"));
                }
                options.map = true;
                Ok(())
            } else if meta.path.is_ident("default") {
                if options.default.is_some() {
                    return Err(meta.error("duplicate confval attribute `default`"));
                }
                if meta.input.peek(syn::Token![=]) {
                    let expr: Expr = meta.value()?.parse()?;
                    options.default = Some(Some(expr));
                } else {
                    options.default = Some(None);
                }
                Ok(())
            } else if meta.path.is_ident("keywords") {
                if options.keywords.is_some() {
                    return Err(meta.error("duplicate confval attribute `keywords`"));
                }
                options.keywords = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("range") {
                if options.range.is_some() {
                    return Err(meta.error("duplicate confval attribute `range`"));
                }
                options.range = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("label") {
                if options.label.is_some() {
                    return Err(meta.error("duplicate confval attribute `label`"));
                }
                options.label = Some(meta.path.clone());
                Ok(())
            } else if meta.path.is_ident("references") {
                if options.references.is_some() {
                    return Err(meta.error("duplicate confval attribute `references`"));
                }
                options.references = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("doc") {
                if confval_doc.is_some() {
                    return Err(meta.error("duplicate confval attribute `doc`"));
                }
                let text: syn::LitStr = meta.value()?.parse()?;
                confval_doc = Some(text.value());
                Ok(())
            } else {
                Err(meta.error(
                    "unknown confval attribute; expected `nested`, `map`, `default`, \
                     `keywords`, `range`, `label`, `references`, or `doc`",
                ))
            }
        })?;
    }
    // A map is not a nested sub-struct, so the two markers cannot both apply to
    // one field.
    if options.map && options.nested {
        return Err(syn::Error::new_spanned(
            field,
            "#[confval(map)] and #[confval(nested)] cannot be combined; \
             a map holds scalar values, not a sub-struct",
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    /// The first field of a parsed struct, the input the attribute reader reads.
    fn first_field(item: syn::ItemStruct) -> Field {
        item.fields
            .into_iter()
            .next()
            .expect("the struct has a field")
    }

    #[test]
    fn keywords_records_the_named_path() {
        // Arrange
        let field = first_field(parse_quote! {
            struct Cfg {
                #[confval(keywords = LimitMode)]
                mode: Located<String>,
            }
        });

        // Act
        let options = parse_options(&field).expect("the attributes read");

        // Assert
        let path = options.keywords.expect("keywords is recorded");
        assert!(path.is_ident("LimitMode"));
        assert!(options.range.is_none());
    }

    #[test]
    fn range_records_the_named_path() {
        // Arrange
        let field = first_field(parse_quote! {
            struct Cfg {
                #[confval(range = PORT)]
                port: Located<i64>,
            }
        });

        // Act
        let options = parse_options(&field).expect("the attributes read");

        // Assert
        let path = options.range.expect("range is recorded");
        assert!(path.is_ident("PORT"));
        assert!(options.keywords.is_none());
    }

    #[test]
    fn a_module_qualified_range_path_is_recorded() {
        // Arrange
        let field = first_field(parse_quote! {
            struct Cfg {
                #[confval(range = limits::MAX_BODY_MB)]
                max_body_mb: Located<i64>,
            }
        });

        // Act
        let options = parse_options(&field).expect("the attributes read");

        // Assert
        let path = options.range.expect("range is recorded");
        assert_eq!(path.segments.len(), 2);
        assert_eq!(path.segments.last().unwrap().ident, "MAX_BODY_MB");
    }

    #[test]
    fn keywords_and_range_are_both_recorded_here() {
        // The reader records both keys. The leaf-type pairing that rejects the
        // combination runs later, in `spec/schema.rs`, where the leaf is known.
        // Arrange
        let field = first_field(parse_quote! {
            struct Cfg {
                #[confval(keywords = LimitMode, range = PORT)]
                mode: Located<String>,
            }
        });

        // Act
        let options = parse_options(&field).expect("the attributes read");

        // Assert
        assert!(options.keywords.is_some());
        assert!(options.range.is_some());
    }

    #[test]
    fn duplicate_keywords_is_an_error() {
        // Arrange
        let field = first_field(parse_quote! {
            struct Cfg {
                #[confval(keywords = LimitMode, keywords = OtherMode)]
                mode: Located<String>,
            }
        });

        // Act
        let error = parse_options(&field)
            .err()
            .expect("a duplicate is rejected");

        // Assert
        assert!(
            error
                .to_string()
                .contains("duplicate confval attribute `keywords`")
        );
    }

    #[test]
    fn duplicate_range_is_an_error() {
        // Arrange
        let field = first_field(parse_quote! {
            struct Cfg {
                #[confval(range = PORT, range = WORKERS)]
                port: Located<i64>,
            }
        });

        // Act
        let error = parse_options(&field)
            .err()
            .expect("a duplicate is rejected");

        // Assert
        assert!(
            error
                .to_string()
                .contains("duplicate confval attribute `range`")
        );
    }
}
