use crate::Frontend;
use crate::frontend::{Absorb, Insert, Recovery, ValueSeparator};
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

    fn line_comments(&self) -> &'static [&'static str] {
        // Strict JSON has no comment.
        &[]
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> Insert {
        // A member alone, with no comma. A missing comma is a visible diagnostic,
        // and a misplaced comma would be a destructive edit, so v0 leaves the
        // comma to the operator. Every member re-renders its opening quote, so
        // the edit absorbs one the operator has already typed.
        let (text, snippet) = match &field.ty {
            // A repeated block is an array of objects, so the insert opens the
            // array with its first element object.
            SchemaType::Block { repeated: true, .. } => {
                (format!("\"{}\": [{{ $0 }}]", field.name), true)
            }
            SchemaType::Block { .. } => (format!("\"{}\": {{\n  $0\n}}", field.name), true),
            SchemaType::StringList => (format!("\"{}\": [$0]", field.name), true),
            SchemaType::StringMap => (format!("\"{}\": {{ $0 }}", field.name), true),
            _ => {
                let placeholder = super::value_placeholder(self, field);
                (
                    format!("\"{}\": {placeholder}", field.name),
                    !placeholder.is_empty(),
                )
            }
        };
        Insert {
            text,
            absorb: Absorb::One(b'"'),
            snippet,
        }
    }

    fn wrap_element(&self, insert: Insert) -> Insert {
        // The `$0` lands the cursor at the value, inside the braces, rather
        // than after the closing brace.
        Insert {
            text: format!("{{ {}$0 }}", insert.text),
            snippet: true,
            ..insert
        }
    }
}
