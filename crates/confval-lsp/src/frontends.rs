//! The three block-structured frontends: HCL, TOML, and KDL.
//!
//! Each binds its `confval` parse function and its insert spelling. Everything
//! else, parsing and resolving a cursor, is the shared default on [`Frontend`].

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

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> String {
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

    fn insert_text(&self, field: &SchemaField, path: &[String]) -> String {
        if is_block(field) {
            if path.is_empty() {
                format!("[{}]", field.name)
            } else {
                format!("[{}.{}]", path.join("."), field.name)
            }
        } else {
            format!("{} = ", field.name)
        }
    }

    fn block_span_covers_body(&self) -> bool {
        // A TOML `[table]` header spans only the header, not its entries, so a
        // table's body extends to the next sibling rather than to the span end.
        false
    }
}

/// The KDL frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Kdl;

impl Frontend for Kdl {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        kdl::parse_kdl_fields(sources, id, report)
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> String {
        if is_block(field) {
            format!("{} {{\n  \n}}", field.name)
        } else {
            format!("{} ", field.name)
        }
    }

    fn attribute_uses_equals(&self) -> bool {
        // KDL writes a node argument as `name value`, with no `=`.
        false
    }

    fn hash_is_comment(&self) -> bool {
        // KDL spells booleans `#true` and `#false`, so `#` is not a comment.
        false
    }
}
