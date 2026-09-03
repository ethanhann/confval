//! `#[derive(Spec)]`'s recorded-check half: the per-field fragments for the
//! generated `ValidateNested::validate_recorded`.
//!
//! Where the schema walk in [`schema`](super::schema) records a field's
//! `#[confval(range = ...)]`, `#[confval(length = ...)]`,
//! `#[confval(format = ...)]`, or `#[confval(keywords = ...)]` constraint for
//! the IR, this walk runs the same constraint during validation, so the
//! attribute is the single source and the author's `Validate` body has
//! no line for it. A scalar leaf emits a `check_located` call, or a
//! `check_format` call for a format, which becomes `check_format_path` on a
//! `PathBuf` leaf. A string list emits a `check_each_in` call for a keyword
//! set or a `check_each_format` call for a format, and both report each bad
//! element at its own span.
//!
//! `#[confval(non_empty)]` and `#[confval(unique)]` are flags rather than
//! value constraints, so each has its own fragment emitted after the
//! constraint fragment. For `non_empty`, a string leaf calls
//! `NON_EMPTY.check_located`, and a string list calls `NON_EMPTY.check_list`
//! for the list and `NON_EMPTY.check_each` for its elements. For `unique`, a
//! string list calls `UNIQUE.check_list`, which reports each repeat at its
//! own span. A flag with `help = "..."` calls the same methods on
//! `NonEmptyConstraint::with_help(...)` or `UniqueConstraint::with_help(...)`. The fragments run in that order, constraint, then `non_empty`,
//! then `unique`, and the renderer keeps that order within one source, so an
//! operator reads a value error before a flag error on the same field.
//!
//! The walk decides what to emit from the one recorded attribute a field
//! has, read through the same `Recorded` classification the schema walk
//! uses, plus the two flags. Which shape may take which attribute is settled
//! in `spec/recorded.rs` and `spec/flags.rs` when the always-emitted
//! `ToSchema` is generated. So is the rule that a field has at most one
//! value constraint. A misplaced or doubled attribute is therefore a compile
//! error before this walk runs. Sharing the classification keeps the two
//! walks from drifting on which attribute means what.

use super::options::FieldOptions;
use super::recorded::{Recorded, one_recording_attribute};
use super::shape::{FieldShape, Leaf};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::Ident;
use syn::ext::IdentExt;

/// The check fragment for one field, or `None` when the field has none
/// of a value constraint, a `non_empty` flag, or a `unique` flag.
///
/// The field name is the config-key string, derived through the same `unraw`
/// form the schema walk uses, so a raw identifier matches the name the manual
/// call passed.
pub(crate) fn field_recorded_check(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
) -> Option<TokenStream2> {
    let name = ident.unraw().to_string();
    let fragments: TokenStream2 = [
        constraint_fragment(ident, shape, options, &name),
        non_empty_fragment(ident, shape, options, &name),
        unique_fragment(ident, shape, options, &name),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!fragments.is_empty()).then_some(fragments)
}

/// The value-constraint fragment for a `range`, a `length`, a `format`, or a
/// `keywords` attribute.
///
/// A required leaf checks `&self.field` directly. An optional leaf checks only
/// when present, through `if let Some`.
///
/// A required leaf with a default gets one more branch. When the value is the
/// default itself, recognized by its detached span and its equality with the
/// declared default, a failed check names the spec's default rather than
/// reporting a config error the operator cannot locate.
fn constraint_fragment(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
    name: &str,
) -> Option<TokenStream2> {
    // The schema walk has already rejected a doubled attribute, so the error
    // arm is unreachable here and the walk reads the one it found.
    let recorded = one_recording_attribute(options).ok().flatten()?;

    // The `check_located` call, given the `&Located<T>` value expression and
    // the report expression it writes into. A `range` names a
    // `RangeConstraint` value, a `length` names a `LengthConstraint` value,
    // a `format` names a type the free function takes as a parameter, and a
    // `keywords` names a `keyword_enum!` type whose `keyword_set()` yields
    // the check. A `format` on a `PathBuf` leaf calls `check_format_path`,
    // which checks the path's text. The reference pass resolves a
    // `references`, because it holds the labels in scope, so this walk emits
    // nothing for one. The match is exhaustive. A constraint added to the
    // schema walk and forgotten here is then a compile error rather than a
    // recorded but unchecked field.
    let is_path = matches!(
        shape,
        FieldShape::Leaf {
            leaf: Leaf::PathBuf,
            ..
        }
    );
    let call = |value: &TokenStream2, report: &TokenStream2| -> Option<TokenStream2> {
        match recorded {
            Recorded::Range(path) | Recorded::Length(path) => {
                Some(quote! { #path.check_located(#value, #name, #report); })
            }
            Recorded::Format(path) if is_path => Some(quote! {
                ::confval::constraints::check_format_path::<#path>(#value, #name, #report);
            }),
            Recorded::Format(path) => Some(quote! {
                ::confval::constraints::check_format::<#path>(#value, #name, #report);
            }),
            Recorded::Keywords(path) => {
                Some(quote! { #path::keyword_set().check_located(#value, #name, #report); })
            }
            Recorded::References(_) => None,
        }
    };

    if matches!(shape, FieldShape::Leaf { optional: true, .. }) {
        let call = call(&quote! { __value }, &quote! { report })?;
        return Some(quote! {
            if let ::core::option::Option::Some(__value) = &self.#ident {
                #call
            }
        });
    }

    // A list records the constraint for one element, so the check runs through
    // `check_each_in` or `check_each_format`, which report each bad element
    // at its own span. Only `keywords` and `format` reach here, because the
    // schema walk refuses `range`, `length`, and `references` on a list. The
    // bare form is already a slice. The optional form keeps the outer
    // `Located`, so the list is reached through its value.
    //
    // Neither arm has the defaulted-value branch a required leaf gets
    // below. A list default is always the empty list, so there is no declared
    // value for the constraint to reject.
    let check_each_call = |values: &TokenStream2| -> Option<TokenStream2> {
        match recorded {
            Recorded::Format(path) => Some(quote! {
                ::confval::constraints::check_each_format::<#path>(#values, #name, report);
            }),
            Recorded::Keywords(path) => {
                Some(quote! { #path::keyword_set().check_each_in(#values, #name, report); })
            }
            Recorded::Range(_) | Recorded::Length(_) | Recorded::References(_) => None,
        }
    };
    if matches!(shape, FieldShape::BareStringList) {
        return check_each_call(&quote! { &self.#ident });
    }
    if matches!(shape, FieldShape::OptionalWrappedStringList) {
        let call = check_each_call(&quote! { &__list.value })?;
        return Some(quote! {
            if let ::core::option::Option::Some(__list) = &self.#ident {
                #call
            }
        });
    }
    let direct = call(&quote! { &self.#ident }, &quote! { report })?;
    let Some(default) = default_expr_typed(shape, options) else {
        return Some(direct);
    };
    let buffered = call(&quote! { &self.#ident }, &quote! { &mut __check })?;
    let prefix = format!("the default for `{name}` fails its recorded constraint: ");
    Some(quote! {
        if self.#ident.span.is_detached() && self.#ident.value == #default {
            let mut __check = ::confval::diagnostic::Report::new();
            #buffered
            for __issue in __check.issues() {
                report
                    .error(::std::format!("{}{}", #prefix, __issue.message))
                    .help(
                        "fix the #[confval(default = ...)] or the recorded constraint"
                            .to_string(),
                    )
                    .emit();
            }
        } else {
            #direct
        }
    })
}

/// The `non_empty` fragment, or `None` when the field is not marked.
///
/// A string leaf calls `check_located`, under `if let Some` when optional. A
/// string list calls `check_list` for the list and `check_each` for its
/// elements. The wrapped list keeps its own span, so the list-level message
/// points at the brackets. The bare `Vec<Located<String>>` holds no span of
/// its own, so its list-level message is reported detached.
fn non_empty_fragment(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
    name: &str,
) -> Option<TokenStream2> {
    options.non_empty.as_ref()?;
    let rule = match &options.non_empty_help {
        Some(help) => quote! { ::confval::constraints::NonEmptyConstraint::with_help(#help) },
        None => quote! { ::confval::constraints::NON_EMPTY },
    };
    let fragment = match shape {
        FieldShape::Leaf {
            leaf: Leaf::String,
            optional: false,
            ..
        } => quote! {
            #rule.check_located(&self.#ident, #name, report);
        },
        FieldShape::Leaf {
            leaf: Leaf::String,
            optional: true,
            ..
        } => quote! {
            if let ::core::option::Option::Some(__value) = &self.#ident {
                #rule.check_located(__value, #name, report);
            }
        },
        FieldShape::BareStringList => quote! {
            #rule.check_list(
                &self.#ident, #name, ::confval::source::Span::detached(), report,
            );
            #rule.check_each(&self.#ident, #name, report);
        },
        FieldShape::OptionalWrappedStringList => quote! {
            if let ::core::option::Option::Some(__list) = &self.#ident {
                #rule.check_list(&__list.value, #name, __list.span, report);
                #rule.check_each(&__list.value, #name, report);
            }
        },
        // `field_schema` runs before this walk and rejects every other shape,
        // so a marked field that reaches here is a `String` leaf or a list.
        _ => unreachable!("the schema walk rejects non_empty on this shape"),
    };
    Some(fragment)
}

/// The `unique` fragment, or `None` when the field is not marked.
///
/// The bare list is a slice already. The wrapped list is reached through its
/// value under `if let Some`. Each repeat is reported at its own span, so no
/// list-level span is needed.
fn unique_fragment(
    ident: &Ident,
    shape: &FieldShape,
    options: &FieldOptions,
    name: &str,
) -> Option<TokenStream2> {
    options.unique.as_ref()?;
    let rule = match &options.unique_help {
        Some(help) => quote! { ::confval::constraints::UniqueConstraint::with_help(#help) },
        None => quote! { ::confval::constraints::UNIQUE },
    };
    let fragment = match shape {
        FieldShape::BareStringList => quote! {
            #rule.check_list(&self.#ident, #name, report);
        },
        FieldShape::OptionalWrappedStringList => quote! {
            if let ::core::option::Option::Some(__list) = &self.#ident {
                #rule.check_list(&__list.value, #name, report);
            }
        },
        // `field_schema` runs before this walk and rejects every other shape,
        // so a marked field that reaches here is a string list.
        _ => unreachable!("the schema walk rejects unique on this shape"),
    };
    Some(fragment)
}

/// The declared default as a typed value expression, or `None` when the field
/// has no default or is not a required leaf. The typed binding pins the
/// expression to the leaf's Rust type, the way the schema walk's default text
/// does.
fn default_expr_typed(shape: &FieldShape, options: &FieldOptions) -> Option<TokenStream2> {
    let FieldShape::Leaf {
        leaf,
        optional: false,
        ..
    } = shape
    else {
        return None;
    };
    let expr = options.default_value()?;
    Some(match leaf {
        Leaf::String => quote! { { let __default: ::std::string::String = #expr; __default } },
        Leaf::Int => quote! { { let __default: i64 = #expr; __default } },
        Leaf::Float => quote! { { let __default: f64 = #expr; __default } },
        Leaf::Bool => quote! { { let __default: bool = #expr; __default } },
        Leaf::PathBuf => quote! { { let __default: ::std::path::PathBuf = #expr; __default } },
    })
}
