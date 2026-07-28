//! TOML write path: serializes a neutral [`Fields`] tree to canonical TOML.
//!
//! This is the inverse of [`parse_toml_fields`](super::parse_toml_fields). It
//! builds a `toml_edit` document by structure and renders it, filling in the
//! doc comments an annotated template carries.

use crate::format::EmitError;
use crate::format::emit::comment_lines;
use crate::format::field::{FieldKind, Fields, Scalar, Value, ValueKind};
use std::collections::HashSet;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value as TomlValue};

/// Serializes a [`Fields`] tree to canonical TOML text.
///
/// This is the inverse of [`parse_toml_fields`](super::parse_toml_fields). It
/// builds a `toml_edit` document by structure and returns its text, dropping the
/// comments and layout the neutral model never held. Same-named blocks at one
/// level group into a `[[array of tables]]`, so a repeated block keeps every
/// element rather than overwriting a sibling. It fails only on a
/// [`ValueKind::Other`], which populate never produces.
pub fn emit_toml(fields: &Fields) -> Result<String, EmitError> {
    let mut document = DocumentMut::new();
    emit_table(fields, document.as_table_mut())?;
    Ok(document.to_string())
}

/// Fills a `toml_edit` table from a neutral level.
fn emit_table(fields: &Fields, table: &mut Table) -> Result<(), EmitError> {
    let mut grouped: HashSet<&str> = HashSet::new();
    for field in fields.iter() {
        match &field.kind {
            FieldKind::Value(value) => {
                table.insert(&field.name, item_of_value(value)?);
                if let Some(doc) = &field.doc
                    && let Some(mut entry) = table.get_key_value_mut(&field.name)
                {
                    entry.0.leaf_decor_mut().set_prefix(toml_comment(doc));
                }
            }
            FieldKind::Block(_) => {
                // Group same-named blocks by name across the level, so a
                // non-consecutive repeat is kept rather than overwritten.
                if !grouped.insert(field.name.as_str()) {
                    continue;
                }
                let blocks: Vec<&Fields> = fields
                    .iter()
                    .filter_map(|other| match &other.kind {
                        FieldKind::Block(inner) if other.name == field.name => Some(inner),
                        _ => None,
                    })
                    .collect();
                if blocks.len() == 1 {
                    let mut subtable = Table::new();
                    emit_table(blocks[0], &mut subtable)?;
                    if let Some(doc) = &field.doc {
                        subtable.decor_mut().set_prefix(toml_block_comment(doc));
                    }
                    table.insert(&field.name, Item::Table(subtable));
                } else {
                    // The comment renders once, above the first array-of-tables
                    // element.
                    let mut array = ArrayOfTables::new();
                    for (index, inner) in blocks.into_iter().enumerate() {
                        let mut subtable = Table::new();
                        emit_table(inner, &mut subtable)?;
                        if index == 0
                            && let Some(doc) = &field.doc
                        {
                            subtable.decor_mut().set_prefix(toml_block_comment(doc));
                        }
                        array.push(subtable);
                    }
                    table.insert(&field.name, Item::ArrayOfTables(array));
                }
            }
        }
    }
    Ok(())
}

/// Maps one neutral value to a table item: a scalar, an inline array, an inline
/// table, or a `[[array of tables]]` for a non-empty sequence of maps.
fn item_of_value(value: &Value) -> Result<Item, EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => Ok(Item::Value(toml_value_of_scalar(scalar))),
        ValueKind::Seq(elements) => {
            if !elements.is_empty()
                && elements
                    .iter()
                    .all(|element| matches!(element.kind, ValueKind::Map(_)))
            {
                let mut array = ArrayOfTables::new();
                for element in elements {
                    if let ValueKind::Map(inner) = &element.kind {
                        let mut subtable = Table::new();
                        emit_table(inner, &mut subtable)?;
                        array.push(subtable);
                    }
                }
                Ok(Item::ArrayOfTables(array))
            } else {
                Ok(Item::Value(TomlValue::Array(toml_array_of(elements)?)))
            }
        }
        ValueKind::Map(inner) => Ok(Item::Value(TomlValue::InlineTable(toml_inline_of(inner)?))),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue(label)),
    }
}

/// Maps one neutral value to an inline `toml_edit` value, for an array element
/// or an inline-table entry. A sequence of maps is an inline array of inline
/// tables here, because an array of tables has no inline spelling.
fn toml_value_of(value: &Value) -> Result<TomlValue, EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => Ok(toml_value_of_scalar(scalar)),
        ValueKind::Seq(elements) => Ok(TomlValue::Array(toml_array_of(elements)?)),
        ValueKind::Map(inner) => Ok(TomlValue::InlineTable(toml_inline_of(inner)?)),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue(label)),
    }
}

fn toml_array_of(elements: &[Value]) -> Result<Array, EmitError> {
    let mut array = Array::new();
    for element in elements {
        array.push(toml_value_of(element)?);
    }
    Ok(array)
}

fn toml_inline_of(fields: &Fields) -> Result<InlineTable, EmitError> {
    let mut inline = InlineTable::new();
    for field in fields.iter() {
        let value = match &field.kind {
            FieldKind::Value(value) => toml_value_of(value)?,
            FieldKind::Block(inner) => TomlValue::InlineTable(toml_inline_of(inner)?),
        };
        inline.insert(&field.name, value);
    }
    Ok(inline)
}

/// Renders a doc comment as TOML comment lines, one `# line` per source line,
/// with a trailing newline so the field follows on its own line. A blank line
/// renders as a bare `#` with no trailing space. TOML content is flat, so the
/// comment carries no indentation.
fn toml_comment(doc: &str) -> String {
    let mut out = String::new();
    for line in comment_lines(doc) {
        if line.is_empty() {
            out.push_str("#\n");
        } else {
            out.push_str("# ");
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// The comment prefix for a `[table]` or a `[[array of tables]]`, with a leading
/// blank line so a commented section keeps the spacing the plain dump has.
fn toml_block_comment(doc: &str) -> String {
    format!("\n{}", toml_comment(doc))
}

fn toml_value_of_scalar(scalar: &Scalar) -> TomlValue {
    match scalar {
        Scalar::String(string) => TomlValue::from(string.clone()),
        Scalar::Int(int) => TomlValue::from(*int),
        Scalar::Float(float) => TomlValue::from(*float),
        Scalar::Bool(boolean) => TomlValue::from(*boolean),
        Scalar::Unparsed(raw) => TomlValue::from(raw.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_toml_fields;
    use super::*;
    use crate::diagnostic::Report;
    use crate::format::field::{Field, FromFields};
    use crate::format::parse::{
        parse_float_field, parse_int_field, parse_string_field, parse_string_list_field,
        parse_struct_field, parse_struct_list_field,
    };
    use crate::source::{Located, SourceMap};

    struct Probe;
    impl FromFields for Probe {
        fn from_fields(_: &Fields, _: &mut Report) -> Option<Self> {
            Some(Probe)
        }
    }

    fn scalar(name: &str, scalar: Scalar) -> Field {
        Field::detached_value(name, Value::detached(ValueKind::Scalar(scalar)))
    }

    fn reparse(text: &str) -> Fields {
        let mut sources = SourceMap::new();
        let id = sources.add("emitted.toml", text.to_string());
        let mut report = Report::new();
        let fields = parse_toml_fields(&sources, id, &mut report).unwrap();
        assert!(
            !report.has_issues(),
            "reparse issues: {:?}",
            report.issues()
        );
        fields
    }

    #[test]
    fn emit_toml_writes_canonical_text() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("hostname", Scalar::String("api".to_string())),
            scalar("port", Scalar::Int(8080)),
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            ),
        ]);
        // Act
        let text = emit_toml(&fields).unwrap();
        // Assert
        assert_eq!(
            text,
            "hostname = \"api\"\nport = 8080\n\n[limits]\nmax_body_mb = 16\n"
        );
    }

    #[test]
    fn emit_toml_round_trips_scalars_and_a_block() {
        let fields = Fields::detached(vec![
            scalar("name", Scalar::String("api".to_string())),
            scalar("count", Scalar::Int(42)),
            scalar("flag", Scalar::Bool(true)),
            Field::detached_block(
                "tls",
                Fields::detached(vec![scalar("cert", Scalar::String("a.pem".to_string()))]),
            ),
        ]);
        let round = reparse(&emit_toml(&fields).unwrap());
        let mut report = Report::new();
        assert_eq!(
            parse_int_field(round.get("count").unwrap(), &mut report)
                .unwrap()
                .value,
            42
        );
        let FieldKind::Block(tls) = &round.get("tls").unwrap().kind else {
            panic!("tls should be a block");
        };
        assert_eq!(
            parse_string_field(tls.get("cert").unwrap(), &mut report)
                .unwrap()
                .value,
            "a.pem"
        );
    }

    #[test]
    fn emit_toml_groups_repeated_blocks_into_array_of_tables() {
        // Two same-named blocks with a field between them still group into one
        // [[service]], so a non-consecutive repeat is not overwritten.
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![
            block(1),
            scalar("name", Scalar::String("x".to_string())),
            block(2),
        ]);
        let text = emit_toml(&fields).unwrap();
        assert!(text.contains("[[service]]"), "got: {text}");
        assert!(!text.contains("[service]\n"), "got: {text}");
        let round = reparse(&text);
        let mut report = Report::new();
        let mut services: Vec<Located<Probe>> = Vec::new();
        parse_struct_list_field(&mut services, round.get("service").unwrap(), &mut report);
        assert_eq!(services.len(), 2);
    }

    #[test]
    fn emit_toml_quotes_a_non_identifier_key() {
        let fields = Fields::detached(vec![scalar("weird key", Scalar::Int(1))]);
        let text = emit_toml(&fields).unwrap();
        assert!(text.contains("\"weird key\""), "got: {text}");
        let round = reparse(&text);
        assert!(round.get("weird key").is_some());
    }

    #[test]
    fn emit_toml_preserves_the_float_distinction() {
        let fields = Fields::detached(vec![
            scalar("ratio", Scalar::Float(0.5)),
            scalar("whole", Scalar::Float(1.0)),
        ]);
        let round = reparse(&emit_toml(&fields).unwrap());
        let mut report = Report::new();
        // A TOML float stays a float, so the int parser rejects it.
        assert!(parse_int_field(round.get("whole").unwrap(), &mut report).is_none());
        assert_eq!(
            parse_float_field(round.get("whole").unwrap(), &mut report)
                .unwrap()
                .value,
            1.0
        );
    }

    #[test]
    fn emit_toml_rejects_an_unrepresentable_value() {
        let fields = Fields::detached(vec![Field::detached_value(
            "when",
            Value::detached(ValueKind::Other("datetime")),
        )]);
        assert_eq!(
            emit_toml(&fields),
            Err(EmitError::UnrepresentableValue("datetime"))
        );
    }

    fn seq_field(name: &str, elements: Vec<ValueKind>) -> Field {
        let values = elements.into_iter().map(Value::detached).collect();
        Field::detached_value(name, Value::detached(ValueKind::Seq(values)))
    }

    #[test]
    fn emit_toml_writes_an_empty_sequence_as_an_inline_array() {
        let fields = Fields::detached(vec![seq_field("allow", vec![])]);
        let text = emit_toml(&fields).unwrap();
        assert_eq!(text, "allow = []\n");
        let round = reparse(&text);
        let mut report = Report::new();
        let list = parse_string_list_field(round.get("allow").unwrap(), &mut report).unwrap();
        assert!(list.value.is_empty());
    }

    #[test]
    fn emit_toml_writes_a_mixed_sequence_as_an_inline_array() {
        // A sequence is an array of tables only when every element is a map, so
        // a scalar mixed in forces an inline array.
        let fields = Fields::detached(vec![seq_field(
            "items",
            vec![
                ValueKind::Scalar(Scalar::Int(1)),
                ValueKind::Map(Fields::detached(vec![scalar("a", Scalar::Int(2))])),
            ],
        )]);
        let text = emit_toml(&fields).unwrap();
        assert!(!text.contains("[[items]]"), "should be inline: {text}");
        assert!(text.contains("items = ["), "got: {text}");
        let round = reparse(&text);
        let FieldKind::Value(value) = &round.get("items").unwrap().kind else {
            panic!("items should be an attribute");
        };
        assert!(matches!(value.kind, ValueKind::Seq(_)));
    }

    #[test]
    fn emit_toml_writes_a_map_as_an_inline_table() {
        let map = Fields::detached(vec![scalar("cert", Scalar::String("a.pem".to_string()))]);
        let fields = Fields::detached(vec![Field::detached_value(
            "tls",
            Value::detached(ValueKind::Map(map)),
        )]);
        let text = emit_toml(&fields).unwrap();
        assert!(text.contains("tls = {"), "got: {text}");
        let round = reparse(&text);
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(round.get("tls").unwrap(), &mut report);
        assert!(parsed.is_some());
    }

    #[test]
    fn emit_toml_round_trips_an_empty_block() {
        let fields = Fields::detached(vec![Field::detached_block(
            "empty",
            Fields::detached(vec![]),
        )]);
        let round = reparse(&emit_toml(&fields).unwrap());
        let FieldKind::Block(inner) = &round.get("empty").unwrap().kind else {
            panic!("empty should be a block");
        };
        assert_eq!(inner.iter().count(), 0);
    }

    #[test]
    fn emit_toml_writes_nested_blocks_as_dotted_headers() {
        let inner = Fields::detached(vec![Field::detached_block(
            "burst",
            Fields::detached(vec![scalar("rate", Scalar::Int(100))]),
        )]);
        let fields = Fields::detached(vec![Field::detached_block("limits", inner)]);
        let text = emit_toml(&fields).unwrap();
        assert!(text.contains("[limits.burst]"), "got: {text}");
        let round = reparse(&text);
        let FieldKind::Block(limits) = &round.get("limits").unwrap().kind else {
            panic!("limits should be a block");
        };
        assert!(matches!(
            limits.get("burst").map(|f| &f.kind),
            Some(FieldKind::Block(_))
        ));
    }

    #[test]
    fn emit_toml_normalizes_a_control_char_in_a_doc_comment() {
        // A NUL is not a printable TOML comment character. Emit drops it, so the
        // template reparses.
        let fields = Fields::detached(vec![
            scalar("port", Scalar::Int(1)).with_doc(Some("a\0b".to_string())),
        ]);
        let text = emit_toml(&fields).unwrap();
        assert!(text.contains("# ab\n"), "got: {text:?}");
        reparse(&text);
    }
}
