//! `#[derive(Spec)]`: generates the structural walks over a spec. It emits an
//! `impl confval::format::FromFields` that parses a spec out of the `Fields` view,
//! the three `impl confval::format::ToFields` walks that write one back, an
//! `impl confval::schema::ToSchema` that describes the type, and the
//! `impl confval::pipeline::ValidateNested` that walks the nested blocks during
//! validation. A struct marked `#[confval(derive_default)]` also gains a
//! generated `Default`.
//!
//! The parser walks the `Fields` view, matches fields by name, reports unknown
//! and missing fields, and builds the struct. It checks no values. Semantic
//! rules are in the `Validate` impl the author writes and in validator
//! functions that operate on the parsed `Located` values.
//!
//! The traversal is generated here rather than under its own derive because it
//! is read off the same field shapes the parser is built from. Both impls come
//! from one classification pass, so the two can never disagree about which
//! fields hold nested specs. See the `traversal` module. The per-field parsing
//! fragments themselves are emitted by the `parser` module.

mod default;
mod flags;
mod options;
mod parser;
mod populate;
mod recorded;
mod schema;
mod shape;
mod source_view;
mod to_fields;
mod traversal;
mod validate_recorded;

use options::{parse_options, parse_struct_options};
use parser::{field_parser, reject_unsupported_default};
use populate::field_emit;
use schema::{field_schema, reject_self_nesting, to_schema_impl};
use shape::classify;
use source_view::field_source_emit;
use to_fields::to_fields_impl;
use traversal::{nested_visit, validate_nested_impl};
use validate_recorded::field_recorded_check;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

/// Builds the `FromFields` parser for one `#[derive(Spec)]` struct.
///
/// The strategy is to walk the struct's fields once and, for each field, decide
/// how it should be parsed and emit the matching code fragments. Those fragments
/// collect into four buckets that are stitched together at the end into a single
/// generated `from_fields` function:
///
/// - `slot_decls`: a local variable per field that holds the value once parsed.
/// - `match_arms`: one arm per field name. When that name is seen in the source,
///   the field is parsed into its slot.
/// - `missing_checks`: run after the walk to report any required field that
///   never appeared.
/// - `constructors`: build the final struct from the filled-in slots.
///
/// At the caller's runtime the generated `from_fields` then iterates the fields
/// present in the config source, routes each by name into its match
/// arm (reporting any unrecognized name), runs the missing-field checks, and
/// constructs `Self`. It checks only shape and presence, never values. Semantic
/// validation happens later, in a separate pass.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    // The derive only handles structs with named fields. Enums and tuple
    // structs are rejected with a message pointing at the type.
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Spec)] supports structs with named fields; \
             write FromFields by hand for enums",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Spec)] requires named fields",
        ));
    };

    let name = &input.ident;
    let struct_options = parse_struct_options(input)?;
    // The four buckets of generated code fragments, filled in below and spliced
    // into the final `impl` at the end.
    let mut slot_decls = Vec::new();
    let mut match_arms = Vec::new();
    let mut missing_checks = Vec::new();
    let mut constructors = Vec::new();
    // Whether any field is the block's label, marked `#[confval(label)]`. A
    // struct with none reports a native label a source wrote as unexpected.
    let mut has_label = false;
    // Two buckets feed the separate `ValidateNested` impl below. `visits` holds
    // one descent per nested field, for `validate_nested`.
    let mut visits = Vec::new();
    // `recorded_checks` holds one constraint check per field that has a
    // `range` or `keywords` attribute, for `validate_recorded`.
    let mut recorded_checks = Vec::new();
    // `#[confval(derive_default)]` fills this with one fragment per field, used
    // to build the generated `Default` impl.
    let mut default_ctors = Vec::new();
    // One fragment per field for each `ToFields` walk: the plain `to_fields`,
    // the source-only `to_source_fields`, and the annotated `to_template`,
    // always built.
    let mut to_fields_emits = Vec::new();
    let mut to_source_emits = Vec::new();
    let mut to_template_emits = Vec::new();
    // One `SchemaField` fragment per field for the always-emitted `ToSchema`
    // walk. Building it here also runs the constraint leaf-pairing check, where
    // the classified shape and leaf are available.
    let mut to_schema_emits = Vec::new();

    for field in &fields.named {
        let ident = field.ident.as_ref().ok_or_else(|| {
            syn::Error::new_spanned(field, "named field is missing an identifier")
        })?;
        let options = parse_options(field)?;
        // A block designates one label field, so a second `#[confval(label)]`
        // is rejected here, the only place that sees every field.
        if let Some(label) = &options.label {
            if has_label {
                return Err(syn::Error::new_spanned(
                    label,
                    "#[confval(label)] marks at most one field",
                ));
            }
            has_label = true;
        }
        let shape = classify(field, options.nested, options.map)?;
        reject_self_nesting(name, &shape)?;
        reject_unsupported_default(field, &shape, &options)?;
        if struct_options.derive_default {
            default_ctors.push(default::field_ctor(ident, &shape, &options)?);
        }

        // A nested field is parsed by the fragments below and validated by the
        // `ValidateNested` impl, so it contributes to both.
        visits.extend(nested_visit(&shape, ident));

        // The populate walks emit one fragment per field, read off the same
        // shape and options, so `ToFields` cannot drift from the parser. The
        // template walk attaches the field's doc comment. The source walk emits
        // only the fields the source set, keyed on the attached span.
        to_fields_emits.push(field_emit(ident, &shape, &options, false));
        to_source_emits.push(field_source_emit(ident, &shape));
        to_template_emits.push(field_emit(ident, &shape, &options, true));
        to_schema_emits.push(field_schema(ident, &shape, &options)?);
        recorded_checks.extend(field_recorded_check(ident, &shape, &options));

        let parsed = field_parser(ident, &shape, &options);
        slot_decls.extend(parsed.slot_decls);
        match_arms.extend(parsed.match_arms);
        missing_checks.extend(parsed.missing_checks);
        constructors.extend(parsed.constructors);
    }

    // A block that designates no label field must not have a native label, so a
    // label a source wrote is reported. A struct with a label field consumes the
    // label in that field's reader instead.
    if !has_label {
        missing_checks.push(quote! {
            if let ::core::option::Option::Some(__label) = fields.label() {
                report
                    .error("a block label is not allowed here")
                    .at(__label.span)
                    .emit();
            }
        });
    }

    let validate_nested = validate_nested_impl(name, &visits, &recorded_checks);
    let default_impl = if struct_options.derive_default {
        default::default_impl(name, &default_ctors)
    } else {
        quote! {}
    };
    let to_fields = to_fields_impl(
        name,
        &to_fields_emits,
        &to_source_emits,
        &to_template_emits,
        &struct_options.doc,
    );
    let to_schema = to_schema_impl(name, &struct_options.doc, &to_schema_emits);

    // This is the code that runs at the caller's runtime, once per parsed
    // struct.
    Ok(quote! {
        impl ::confval::format::FromFields for #name {
            fn from_fields(
                fields: &::confval::format::Fields,
                report: &mut ::confval::diagnostic::Report,
            ) -> ::core::option::Option<Self> {
                #(#slot_decls)*

                // `iter` yields the fields a configuration sets. A commented
                // entry is a template's rendering of one it does not, so the
                // walk never sees it and never reports it as unknown.
                for __field in fields.iter() {
                    match __field.name.as_str() {
                        #(#match_arms)*
                        _ => ::confval::format::report_unknown_field(__field, report),
                    }
                }

                #(#missing_checks)*

                ::core::option::Option::Some(Self {
                    #(#constructors)*
                })
            }
        }

        #validate_nested

        #default_impl

        #to_fields

        #to_schema
    })
}
