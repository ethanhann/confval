use crate::Frontend;
use crate::frontend::{Recovery, ValueSeparator};
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::json as format_json;
use confval::schema::{SchemaField, SchemaType};
use confval::source::{SourceId, SourceMap};

/// The JSON frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Json;

impl Frontend for Json {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        format_json::parse_json_fields(sources, id, report)
    }

    fn recovery(&self) -> Recovery {
        // JSON nests through object braces and array brackets, with quoted keys.
        Recovery::Object
    }

    fn value_separator(&self) -> ValueSeparator {
        // JSON writes a member as `"key": value`.
        ValueSeparator::Colon
    }

    fn hash_is_comment(&self) -> bool {
        // Strict JSON has no comment, so `#` is an ordinary character.
        false
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> String {
        // A member alone, with no comma. A missing comma is a visible diagnostic,
        // and a misplaced comma would be a destructive edit, so v0 leaves the
        // comma to the operator.
        match &field.ty {
            // A repeated block is an array of objects, so the insert opens the
            // array with its first element object.
            SchemaType::Block { repeated: true, .. } => {
                format!("\"{}\": [{{ $0 }}]", field.name)
            }
            SchemaType::Block { .. } => format!("\"{}\": {{\n  $0\n}}", field.name),
            _ => format!("\"{}\": ", field.name),
        }
    }
}
