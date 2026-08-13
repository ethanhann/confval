use crate::Frontend;
use crate::frontend::Recovery;
use crate::frontends::is_block;
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::toml as format_toml;
use confval::schema::SchemaField;
use confval::source::{SourceId, SourceMap};

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

    fn recovery(&self) -> Recovery {
        // TOML addresses a table by a `[header]`, so the text recovery
        // reconstructs the path from the last header rather than open braces.
        Recovery::Header
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

#[cfg(test)]
mod tests {
    use super::*;
    use confval::schema::{Schema, SchemaType};

    fn block(name: &str) -> SchemaField {
        SchemaField::new(
            name.to_string(),
            None,
            true,
            false,
            SchemaType::Block {
                schema: Box::new(Schema::new(None, Vec::new())),
                repeated: false,
            },
        )
    }

    #[test]
    fn a_root_block_completes_as_a_plain_header() {
        // Arrange, Act
        let header = Toml.insert_text(&block("limits"), &[]);

        // Assert
        assert_eq!(header, "[limits]");
    }

    #[test]
    fn a_nested_block_completes_as_a_qualified_header() {
        // Arrange, Act
        let header = Toml.insert_text(&block("sub"), &["limits".to_string()]);

        // Assert
        assert_eq!(header, "[limits.sub]");
    }
}
