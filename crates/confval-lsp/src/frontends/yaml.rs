use crate::Frontend;
use crate::frontend::{Recovery, ValueSeparator};
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::yaml as format_yaml;
use confval::schema::{SchemaField, SchemaType};
use confval::source::{SourceId, SourceMap};

/// The YAML frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Yaml;

impl Frontend for Yaml {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        format_yaml::parse_yaml_fields(sources, id, report)
    }

    fn recovery(&self) -> Recovery {
        // Block YAML nests by indentation, so resolution reads the raw text in
        // both parse states.
        Recovery::Indentation
    }

    fn value_separator(&self) -> ValueSeparator {
        // YAML writes a member as `key: value`.
        ValueSeparator::Colon
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> String {
        match &field.ty {
            // A repeated block is a sequence, so the insert opens the first
            // element with a `-` marker.
            SchemaType::Block { repeated: true, .. } => format!("{}:\n  - $0", field.name),
            // A single nested mapping opens its body on the next indented line.
            SchemaType::Block { .. } => format!("{}:\n  $0", field.name),
            _ => format!("{}: ", field.name),
        }
    }
}
