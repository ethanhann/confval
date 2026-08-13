use crate::Frontend;
use crate::frontends::is_block;
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::hcl as format_hcl;
use confval::schema::SchemaField;
use confval::source::{SourceId, SourceMap};

/// The HCL frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Hcl;

impl Frontend for Hcl {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        format_hcl::parse_hcl_fields(sources, id, report)
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> String {
        if is_block(field) {
            format!("{} {{\n  \n}}", field.name)
        } else {
            format!("{} = ", field.name)
        }
    }
}
