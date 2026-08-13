use crate::Frontend;
use crate::frontends::is_block;
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::kdl as format_kdl;
use confval::schema::SchemaField;
use confval::source::{SourceId, SourceMap};

/// The KDL frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Kdl;

impl Frontend for Kdl {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        format_kdl::parse_kdl_fields(sources, id, report)
    }

    fn attribute_uses_equals(&self) -> bool {
        // KDL writes a node argument as `name value`, with no `=`.
        false
    }

    fn hash_is_comment(&self) -> bool {
        // KDL spells booleans `#true` and `#false`, so `#` is not a comment.
        false
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> String {
        if is_block(field) {
            format!("{} {{\n  \n}}", field.name)
        } else {
            format!("{} ", field.name)
        }
    }
}
