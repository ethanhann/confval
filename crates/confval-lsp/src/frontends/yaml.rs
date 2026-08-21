use crate::Frontend;
use crate::frontend::{Insert, Recovery, ValueSeparator};
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

    fn line_comments(&self) -> &'static [&'static str] {
        // The YAML reader handles its own comments, whitespace-preceded, so
        // this vocabulary serves only the trait's contract.
        &["#"]
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> Insert {
        match &field.ty {
            // A repeated block and a string list are both sequences, so the
            // insert opens the first element with a `-` marker.
            SchemaType::Block { repeated: true, .. } | SchemaType::StringList { .. } => {
                Insert::snippet(format!("{}:\n  - $0", field.name))
            }
            // A single nested mapping and a map both open a body on the next
            // indented line.
            SchemaType::Block { .. } | SchemaType::StringMap => {
                Insert::snippet(format!("{}:\n  $0", field.name))
            }
            _ => {
                let placeholder = super::value_placeholder(self, field);
                super::scalar_insert(format!("{}: {placeholder}", field.name), &placeholder)
            }
        }
    }

    fn wrap_element(&self, insert: Insert) -> Insert {
        Insert {
            text: format!("- {}", insert.text),
            ..insert
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::ValueSeparator;
    use confval::schema::{ScalarType, Schema, SchemaType};

    fn field(name: &str, ty: SchemaType) -> SchemaField {
        SchemaField::new(name.to_string(), None, ty).required()
    }

    fn block_type(repeated: bool) -> SchemaType {
        SchemaType::block(Schema::new(None, Vec::new()), repeated)
    }

    #[test]
    fn the_line_comment_vocabulary_is_the_hash() {
        // Arrange, Act
        let comments = Yaml.line_comments();

        // Assert
        assert_eq!(comments, ["#"]);
    }

    #[test]
    fn the_value_separator_is_a_colon() {
        // Arrange, Act
        let separator = Yaml.value_separator();

        // Assert
        assert_eq!(separator, ValueSeparator::Colon);
    }

    #[test]
    fn a_repeated_block_opens_a_sequence_element() {
        // Arrange, Act
        let insert = Yaml.insert_text(&field("rules", block_type(true)), &[]);

        // Assert
        assert_eq!(insert.text, "rules:\n  - $0");
    }

    #[test]
    fn a_string_list_opens_a_sequence_element() {
        // Arrange, Act
        let insert = Yaml.insert_text(&field("tags", SchemaType::string_list(None)), &[]);

        // Assert
        assert_eq!(insert.text, "tags:\n  - $0");
    }

    #[test]
    fn a_single_block_opens_an_indented_body() {
        // Arrange, Act
        let insert = Yaml.insert_text(&field("limits", block_type(false)), &[]);

        // Assert
        assert_eq!(insert.text, "limits:\n  $0");
    }

    #[test]
    fn a_string_map_opens_an_indented_body() {
        // Arrange, Act
        let insert = Yaml.insert_text(&field("labels", SchemaType::StringMap), &[]);

        // Assert
        assert_eq!(insert.text, "labels:\n  $0");
    }

    #[test]
    fn a_scalar_writes_a_key_and_a_space() {
        // Arrange, Act
        let insert = Yaml.insert_text(
            &field(
                "port",
                SchemaType::Scalar {
                    leaf: ScalarType::Int,
                    constraint: None,
                },
            ),
            &[],
        );

        // Assert
        assert_eq!(insert.text, "port: ");
    }
}
