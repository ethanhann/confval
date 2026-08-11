//! Emitting the per-field `FromFields` parsing fragments for `#[derive(Spec)]`.
//!
//! The parent module walks a struct's fields once and, for each field, calls
//! [`field_parser`] to turn one [`FieldShape`] and its [`FieldOptions`] into the
//! four code fragments the generated `from_fields` is stitched from: a slot
//! declaration, a match arm keyed by the field name, an optional missing-field
//! check, and a constructor entry.
//!
//! This reads the same `FieldShape` the write half in
//! [`populate`](super::populate) reads, so the parser and the populate walk
//! cannot disagree about a field's shape.

use super::options::FieldOptions;
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Field, Ident};

/// The four generated fragments for one field, each spliced into the matching
/// bucket in the parent's `from_fields`.
pub(crate) struct FieldParser {
    pub slot_decls: Vec<TokenStream2>,
    pub match_arms: Vec<TokenStream2>,
    pub missing_checks: Vec<TokenStream2>,
    pub constructors: Vec<TokenStream2>,
}

/// Emits the parsing fragments for one field, tailored to its shape.
///
/// `slot` is the generated local variable's name, e.g. `__port`. The leading
/// underscores keep it from clashing with the user's own names. A required field
/// with no default also gets a `__<name>_seen` boolean, so a present-but-failed
/// field is not also reported as missing without an `O(fields)` `Fields::has`
/// rescan.
pub(crate) fn field_parser(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> FieldParser {
    // A field whose config key is a Rust keyword is a raw identifier, so strip
    // the `r#` before it becomes the matched name. The slot and seen locals go
    // through `format_ident!`, which already unraws, so they are not touched.
    let field_name = ident.unraw().to_string();
    let slot = format_ident!("__{}", ident);
    let mut out = FieldParser {
        slot_decls: Vec::new(),
        match_arms: Vec::new(),
        missing_checks: Vec::new(),
        constructors: Vec::new(),
    };

    match shape {
        FieldShape::Leaf { leaf, optional } => {
            let parser = leaf_parser(leaf);
            // Every leaf is single-occurrence. The walk parses the first
            // occurrence and reports a repeat as a duplicate pointing back at
            // it, so a repeated scalar is never silently overwritten. The seen
            // span doubles as the presence marker for the missing-field check.
            let seen = format_ident!("__{}_seen", ident);
            out.slot_decls.push(quote! {
                let mut #slot = ::core::option::Option::None;
                let mut #seen: ::core::option::Option<::confval::source::Span> =
                    ::core::option::Option::None;
            });
            out.match_arms.push(quote! {
                #field_name => {
                    if ::confval::format::first_occurrence(
                        &mut #seen, #field_name, __field, report,
                    ) {
                        #slot = #parser;
                    }
                }
            });
            match (options.default_value(), optional) {
                (Some(expr), true) => {
                    out.constructors.push(quote! {
                        #ident: #slot.or_else(|| ::core::option::Option::Some(
                            ::confval::source::Located::detached(#expr),
                        )),
                    });
                }
                (Some(expr), false) => {
                    out.constructors.push(quote! {
                        #ident: #slot.unwrap_or_else(
                            || ::confval::source::Located::detached(#expr),
                        ),
                    });
                }
                (None, true) => {
                    out.constructors.push(quote! { #ident: #slot, });
                }
                (None, false) => {
                    out.missing_checks
                        .push(seen_missing_check(&field_name, &seen));
                    out.constructors.push(quote! { #ident: #slot?, });
                }
            }
        }
        FieldShape::BareStringList => {
            // A string list accumulates same-named occurrences in document
            // order, so the repeated-node list form keeps every element.
            out.slot_decls
                .push(quote! { let mut #slot = ::core::option::Option::None; });
            if options.default.is_some() {
                out.match_arms.push(quote! {
                    #field_name => ::confval::format::parse_string_list_occurrence(
                        &mut #slot, __field, report,
                    ),
                });
                out.constructors.push(quote! {
                    #ident: #slot.map(|__list| __list.value).unwrap_or_default(),
                });
            } else {
                let seen = format_ident!("__{}_seen", ident);
                out.slot_decls.push(quote! {
                    let mut #seen: ::core::option::Option<::confval::source::Span> =
                        ::core::option::Option::None;
                });
                out.match_arms.push(quote! {
                    #field_name => {
                        #seen = #seen.or(::core::option::Option::Some(__field.span));
                        ::confval::format::parse_string_list_occurrence(
                            &mut #slot, __field, report,
                        );
                    }
                });
                out.missing_checks
                    .push(seen_missing_check(&field_name, &seen));
                out.constructors.push(quote! { #ident: #slot?.value, });
            }
        }
        FieldShape::OptionalWrappedStringList => {
            out.slot_decls
                .push(quote! { let mut #slot = ::core::option::Option::None; });
            out.match_arms.push(quote! {
                #field_name => ::confval::format::parse_string_list_occurrence(
                    &mut #slot, __field, report,
                ),
            });
            out.constructors.push(quote! { #ident: #slot, });
        }
        FieldShape::Nested { optional, .. } => {
            let seen = format_ident!("__{}_seen", ident);
            out.slot_decls.push(quote! {
                let mut #slot = ::core::option::Option::None;
                let mut #seen: ::core::option::Option<::confval::source::Span> =
                    ::core::option::Option::None;
            });
            out.match_arms.push(quote! {
                #field_name => ::confval::format::parse_single_struct(
                    &mut #slot, &mut #seen, #field_name, __field, report,
                ),
            });
            if *optional {
                out.constructors.push(quote! { #ident: #slot, });
            } else {
                // A non-optional nested field is a `Located<S>`. With
                // `#[confval(default)]` an absent block is filled with
                // `S::default()` and is not reported as missing. Without the
                // attribute, absence is a missing-field error. Either way a
                // present-but-failed child is replaced with a detached default so
                // the parent and its siblings still validate. The child's
                // structural error is already in the report, so the lowering gate
                // blocks before the placeholder reaches runtime.
                if options.default.is_none() {
                    out.missing_checks.push(quote! {
                        if #seen.is_none() {
                            ::confval::format::report_missing_field(
                                #field_name, fields.enclosing(), report,
                            );
                        }
                    });
                }
                out.constructors.push(quote! {
                    #ident: #slot.unwrap_or_default(),
                });
            }
        }
        FieldShape::NestedList { .. } => {
            out.slot_decls
                .push(quote! { let mut #slot = ::std::vec::Vec::new(); });
            out.match_arms.push(quote! {
                #field_name => ::confval::format::parse_struct_list_field(
                    &mut #slot, __field, report,
                ),
            });
            out.constructors.push(quote! { #ident: #slot, });
        }
        FieldShape::Map => {
            // A map is single-occurrence, like a leaf or a nested block. The
            // seen span records the first occurrence, so a repeated map field
            // is a duplicate and a present-but-failed map is not also reported
            // missing.
            let seen = format_ident!("__{}_seen", ident);
            out.slot_decls.push(quote! {
                let mut #slot = ::core::option::Option::None;
                let mut #seen: ::core::option::Option<::confval::source::Span> =
                    ::core::option::Option::None;
            });
            out.match_arms.push(quote! {
                #field_name => {
                    if ::confval::format::first_occurrence(
                        &mut #seen, #field_name, __field, report,
                    ) {
                        #slot = ::confval::format::parse_string_map_field(__field, report);
                    }
                }
            });
            if options.default.is_some() {
                out.constructors.push(quote! {
                    #ident: #slot.map(|__map| __map.value).unwrap_or_default(),
                });
            } else {
                out.missing_checks
                    .push(seen_missing_check(&field_name, &seen));
                out.constructors.push(quote! { #ident: #slot?.value, });
            }
        }
    }

    out
}

/// Picks the confval parse function for a leaf type.
///
/// Returns a generated expression that parses the current field into an
/// `Option<Located<T>>`. Each arm names the confval parser for that leaf, so a
/// generated parser and a handwritten one read a field the same way.
fn leaf_parser(leaf: &Leaf) -> TokenStream2 {
    match leaf {
        Leaf::String => quote! { ::confval::format::parse_string_field(__field, report) },
        Leaf::Int => quote! { ::confval::format::parse_int_field(__field, report) },
        Leaf::Float => quote! { ::confval::format::parse_float_field(__field, report) },
        Leaf::Bool => quote! { ::confval::format::parse_bool_field(__field, report) },
        Leaf::PathBuf => quote! { ::confval::format::parse_path_field(__field, report) },
    }
}

/// The generated after-the-walk check that reports a required field as missing.
///
/// `seen` is the span local the match arm fills when it first parses the
/// field. If the walk finished without setting it, the field was absent
/// and an error is reported against the enclosing block.
fn seen_missing_check(field_name: &str, seen: &Ident) -> TokenStream2 {
    quote! {
        if #seen.is_none() {
            ::confval::format::report_missing_field(#field_name, fields.enclosing(), report);
        }
    }
}

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
