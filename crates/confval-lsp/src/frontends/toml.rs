use crate::Frontend;
use crate::frontends::is_block;
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::toml as format_toml;
use confval::prelude::SourceMap;
use confval::schema::SchemaField;
use confval::source::SourceId;

/// The TOML frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Toml;

impl Frontend for Toml {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        format_toml::parse_toml_fields(sources, id, report)
    }

    fn block_span_covers_body(&self) -> bool {
        // A TOML `[table]` header spans only the header, not its entries, so a
        // table's body extends to the next sibling rather than to the span end.
        false
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
}
