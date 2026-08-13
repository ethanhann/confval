//! The three block-structured frontends: HCL, TOML, and KDL.
//!
//! Each binds its `confval` parse function and its insert spelling. Everything
//! else, parsing into the retained tree and resolving a cursor, is the shared
//! default on [`Frontend`].

use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::{hcl, kdl, toml};
use confval::schema::{SchemaField, SchemaType};
use confval::source::{SourceId, SourceMap};

use crate::frontend::Frontend;

/// Whether a field is written as a block rather than an attribute.
fn is_block(field: &SchemaField) -> bool {
    matches!(field.ty, SchemaType::Block { .. })
}

/// The HCL frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Hcl;

impl Frontend for Hcl {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        hcl::parse_hcl_fields(sources, id, report)
    }

    fn insert_text(&self, field: &SchemaField) -> String {
        if is_block(field) {
            format!("{} {{\n  \n}}", field.name)
        } else {
            format!("{} = ", field.name)
        }
    }
}

/// The TOML frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Toml;

impl Frontend for Toml {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        toml::parse_toml_fields(sources, id, report)
    }

    fn insert_text(&self, field: &SchemaField) -> String {
        if is_block(field) {
            format!("[{}]", field.name)
        } else {
            format!("{} = ", field.name)
        }
    }
}

/// The KDL frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Kdl;

impl Frontend for Kdl {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        kdl::parse_kdl_fields(sources, id, report)
    }

    fn insert_text(&self, field: &SchemaField) -> String {
        if is_block(field) {
            format!("{} {{\n  \n}}", field.name)
        } else {
            format!("{} ", field.name)
        }
    }
}
