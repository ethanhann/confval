use crate::Frontend;
use crate::frontend::{Insert, ValueSeparator, quoted_literal};
use crate::frontends::is_block;
use confval::diagnostic::Report;
use confval::format::Fields;
use confval::format::kdl as format_kdl;
use confval::schema::{ScalarType, SchemaField, SchemaType};
use confval::source::{SourceId, SourceMap};

/// The KDL frontend.
#[derive(Debug, Default, Clone, Copy)]
pub struct Kdl;

impl Frontend for Kdl {
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
        format_kdl::parse_kdl_fields(sources, id, report)
    }

    fn value_separator(&self) -> ValueSeparator {
        // KDL writes a node argument as `name value`, with no `=`.
        ValueSeparator::Whitespace
    }

    fn default_literal(&self, leaf: &ScalarType, text: &str) -> String {
        // KDL writes booleans `#true` and `#false`.
        match leaf {
            ScalarType::Bool => format!("#{text}"),
            ScalarType::String | ScalarType::Path => quoted_literal(text),
            _ => text.to_string(),
        }
    }

    fn line_comments(&self) -> &'static [&'static str] {
        // KDL writes booleans `#true` and `#false`, so `#` is not a comment.
        &["//"]
    }

    fn insert_text(&self, field: &SchemaField, _path: &[String]) -> Insert {
        // A KDL map is written as a children block, like a nested block, so
        // both open the braces and land the cursor inside.
        let block_form = is_block(field) || matches!(field.ty, SchemaType::StringMap);
        if block_form {
            Insert::snippet(format!("{} {{\n  $0\n}}", field.name))
        } else {
            let placeholder = super::value_placeholder(self, field);
            super::scalar_insert(format!("{} {placeholder}", field.name), &placeholder)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confval::schema::{Schema, SchemaType};

    #[test]
    fn a_string_map_opens_a_children_block() {
        // Arrange
        // A KDL map is written as a children block, so the insert opens the
        // braces and lands the cursor inside.
        let field =
            SchemaField::new("headers".to_string(), None, SchemaType::StringMap).with_default();

        // Act
        let insert = Kdl.insert_text(&field, &[]);

        // Assert
        assert_eq!(insert.text, "headers {\n  $0\n}");
    }

    #[test]
    fn a_nested_block_opens_a_children_block() {
        // Arrange
        let field = SchemaField::new(
            "limits".to_string(),
            None,
            SchemaType::Block {
                schema: Box::new(Schema::new(None, Vec::new())),
                repeated: false,
            },
        );

        // Act
        let insert = Kdl.insert_text(&field, &[]);

        // Assert
        assert_eq!(insert.text, "limits {\n  $0\n}");
    }
}
