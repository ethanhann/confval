use crate::Frontend;
use crate::frontend::Insert;
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::hcl as format_hcl;
use confval::schema::{SchemaField, SchemaType};
use confval::source::{SourceId, SourceMap};

/// The HCL frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Hcl;

impl Frontend for Hcl {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        format_hcl::parse_hcl_fields(sources, id, report)
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> Insert {
        Insert::plain(match &field.ty {
            SchemaType::Block { .. } => format!("{} {{\n  $0\n}}", field.name),
            SchemaType::StringList => format!("{} = [$0]", field.name),
            SchemaType::StringMap => format!("{} = {{ $0 }}", field.name),
            _ => format!("{} = {}", field.name, super::value_placeholder(self, field)),
        })
    }
}
