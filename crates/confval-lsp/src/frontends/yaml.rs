use crate::Frontend;
use crate::frontend::{Recovery, ValueSeparator};
use crate::frontends::is_block;
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::yaml as format_yaml;
use confval::schema::SchemaField;
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
        // A nested mapping opens its body on the next indented line. The cursor
        // lands on that line at the tab stop.
        if is_block(field) {
            format!("{}:\n  $0", field.name)
        } else {
            format!("{}: ", field.name)
        }
    }
}
