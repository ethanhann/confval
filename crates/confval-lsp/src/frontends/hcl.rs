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
        match &field.ty {
            SchemaType::Block { .. } => Insert::snippet(format!("{} {{\n  $0\n}}", field.name)),
            SchemaType::StringList { .. } => Insert::snippet(format!("{} = [$0]", field.name)),
            SchemaType::StringMap => Insert::snippet(format!("{} = {{ $0 }}", field.name)),
            _ => {
                let placeholder = super::value_placeholder(self, field);
                super::scalar_insert(format!("{} = {placeholder}", field.name), &placeholder)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confval::schema::{ScalarType, Schema, SchemaType};

    fn field(name: &str, ty: SchemaType) -> SchemaField {
        SchemaField::new(name.to_string(), None, ty).required()
    }

    fn block_type() -> SchemaType {
        SchemaType::block(Schema::new(None, Vec::new()), false)
    }

    #[test]
    fn a_block_opens_a_brace_body() {
        // Arrange, Act
        let insert = Hcl.insert_text(&field("limits", block_type()), &[]);

        // Assert
        assert_eq!(insert.text, "limits {\n  $0\n}");
        assert!(insert.snippet);
    }

    #[test]
    fn a_string_list_opens_an_array() {
        // Arrange, Act
        let insert = Hcl.insert_text(&field("allow", SchemaType::string_list(None)), &[]);

        // Assert
        assert_eq!(insert.text, "allow = [$0]");
    }

    #[test]
    fn a_string_map_opens_an_inline_object() {
        // Arrange, Act
        let insert = Hcl.insert_text(&field("headers", SchemaType::StringMap), &[]);

        // Assert
        assert_eq!(insert.text, "headers = { $0 }");
    }

    #[test]
    fn a_scalar_writes_a_key_and_equals() {
        // Arrange, Act
        let insert = Hcl.insert_text(
            &field("port", SchemaType::scalar(ScalarType::Int, None)),
            &[],
        );

        // Assert
        assert_eq!(insert.text, "port = ");
        assert!(!insert.snippet);
    }
}
