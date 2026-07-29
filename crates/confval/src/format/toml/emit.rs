//! TOML write path: serializes a neutral [`Fields`] tree to canonical TOML.
//!
//! This is the inverse of [`parse_toml_fields`](super::parse_toml_fields). It
//! builds a `toml_edit` document by structure and renders it, filling in the
//! doc comments an annotated template carries.

use crate::format::EmitError;
use crate::format::emit::{child_path, comment_lines};
use crate::format::field::{FieldKind, Fields, Scalar, Value, ValueKind};
use std::collections::HashSet;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value as TomlValue};

/// Serializes a [`Fields`] tree to canonical TOML text.
///
/// This is the inverse of [`parse_toml_fields`](super::parse_toml_fields). It
/// builds a `toml_edit` document by structure and returns its text, dropping the
/// comments and layout the neutral model never held. Same-named blocks at one
/// level group into a `[[array of tables]]`, so a repeated block keeps every
/// element rather than overwriting a sibling. It fails on a
/// [`ValueKind::Other`] and on a name with conflicting uses at one level: a
/// value next to a same-named block, two same-named values, or any repetition
/// inside an inline table. Populate produces none of these, so they arise only
/// for a parsed or hand-built `Fields`.
pub fn emit_toml(fields: &Fields) -> Result<String, EmitError> {
    let mut document = DocumentMut::new();
    emit_table(fields, document.as_table_mut(), "")?;
    let text = document.to_string();
    // A doc-commented section carries a leading blank line to separate it from
    // what precedes it. At the top of the document nothing precedes it.
    match text.strip_prefix('\n') {
        Some(stripped) => Ok(stripped.to_string()),
        None => Ok(text),
    }
}

/// A name whose uses at one level TOML cannot spell side by side. Repeated
/// blocks group into an array of tables, so a name repeated only by blocks is
/// fine. Any other repetition, two values or a value next to a block, would
/// silently overwrite one of them, and emit refuses instead.
fn conflicting_name(fields: &Fields) -> Option<&str> {
    fields.iter().find_map(|field| {
        let group = fields.iter().filter(|other| other.name == field.name);
        let mut count = 0;
        let mut all_blocks = true;
        for other in group {
            count += 1;
            all_blocks &= matches!(other.kind, FieldKind::Block(_));
        }
        (count > 1 && !all_blocks).then_some(field.name.as_str())
    })
}

/// A name repeated at all inside an inline table, where nothing can repeat,
/// not even blocks, which have no array-of-tables spelling there.
fn repeated_inline_name(fields: &Fields) -> Option<&str> {
    fields.iter().find_map(|field| {
        let count = fields
            .iter()
            .filter(|other| other.name == field.name)
            .count();
        (count > 1).then_some(field.name.as_str())
    })
}

/// Fills a `toml_edit` table from a neutral level.
fn emit_table(fields: &Fields, table: &mut Table, path: &str) -> Result<(), EmitError> {
    if let Some(name) = conflicting_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let mut grouped: HashSet<&str> = HashSet::new();
    for field in fields.iter() {
        match &field.kind {
            FieldKind::Value(value) => {
                let child = child_path(path, &field.name);
                let mut item = item_of_value(value, &child)?;
                // An array of tables renders its key once per `[[element]]`,
                // so a comment on the key would repeat. It goes above the
                // first element instead, like the block path's grouped form.
                if let Item::ArrayOfTables(array) = &mut item {
                    if let (Some(doc), Some(first)) = (&field.doc, array.iter_mut().next()) {
                        first.decor_mut().set_prefix(toml_block_comment(doc));
                    }
                    table.insert(&field.name, item);
                } else {
                    table.insert(&field.name, item);
                    if let Some(doc) = &field.doc
                        && let Some(mut entry) = table.get_key_value_mut(&field.name)
                    {
                        entry.0.leaf_decor_mut().set_prefix(toml_comment(doc));
                    }
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
                // Only one comment can render above the group, so the group
                // takes the first doc any element carries.
                let doc = fields
                    .iter()
                    .filter(|other| other.name == field.name)
                    .find_map(|other| other.doc.as_deref());
                let child = child_path(path, &field.name);
                if blocks.len() == 1 {
                    let mut subtable = Table::new();
                    emit_table(blocks[0], &mut subtable, &child)?;
                    if let Some(doc) = doc {
                        subtable.decor_mut().set_prefix(toml_block_comment(doc));
                    }
                    table.insert(&field.name, Item::Table(subtable));
                } else {
                    // The comment renders once, above the first array-of-tables
                    // element.
                    let mut array = ArrayOfTables::new();
                    for (index, inner) in blocks.into_iter().enumerate() {
                        let mut subtable = Table::new();
                        emit_table(inner, &mut subtable, &child)?;
                        if index == 0
                            && let Some(doc) = doc
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
fn item_of_value(value: &Value, path: &str) -> Result<Item, EmitError> {
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
                        emit_table(inner, &mut subtable, path)?;
                        array.push(subtable);
                    }
                }
                Ok(Item::ArrayOfTables(array))
            } else {
                Ok(Item::Value(TomlValue::Array(toml_array_of(
                    elements, path,
                )?)))
            }
        }
        ValueKind::Map(inner) => Ok(Item::Value(TomlValue::InlineTable(toml_inline_of(
            inner, path,
        )?))),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// Maps one neutral value to an inline `toml_edit` value, for an array element
/// or an inline-table entry. A sequence of maps is an inline array of inline
/// tables here, because an array of tables has no inline spelling.
fn toml_value_of(value: &Value, path: &str) -> Result<TomlValue, EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => Ok(toml_value_of_scalar(scalar)),
        ValueKind::Seq(elements) => Ok(TomlValue::Array(toml_array_of(elements, path)?)),
        ValueKind::Map(inner) => Ok(TomlValue::InlineTable(toml_inline_of(inner, path)?)),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

fn toml_array_of(elements: &[Value], path: &str) -> Result<Array, EmitError> {
    let mut array = Array::new();
    for element in elements {
        array.push(toml_value_of(element, path)?);
    }
    Ok(array)
}

fn toml_inline_of(fields: &Fields, path: &str) -> Result<InlineTable, EmitError> {
    if let Some(name) = repeated_inline_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let mut inline = InlineTable::new();
    for field in fields.iter() {
        let child = child_path(path, &field.name);
        let value = match &field.kind {
            FieldKind::Value(value) => toml_value_of(value, &child)?,
            FieldKind::Block(inner) => TomlValue::InlineTable(toml_inline_of(inner, &child)?),
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
    fn emit_toml_rejects_a_value_and_a_block_sharing_a_name() {
        // Arrange
        // HCL spells `x = 1` next to `x { }`, so a parsed Fields can hold
        // both. A TOML key names one thing, so emitting the pair would lose
        // one of them silently.
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![scalar("y", Scalar::Int(2))])),
        ]);

        // Act
        let result = emit_toml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "".to_string(),
            })
        );
    }

    #[test]
    fn emit_toml_rejects_the_shared_name_inside_an_inline_table() {
        // Arrange
        let pair = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![])),
        ]);
        let fields = Fields::detached(vec![Field::detached_value(
            "obj",
            Value::detached(ValueKind::Map(pair)),
        )]);

        // Act
        let result = emit_toml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "obj".to_string(),
            })
        );
    }

    #[test]
    fn emit_toml_writes_an_array_of_tables_doc_once() {
        // Arrange
        // The key of an array of tables renders once per `[[element]]`, so a
        // comment on the key would repeat. It belongs above the first element,
        // matching the block path's pinned behavior.
        let map = |port: i64| {
            Value::detached(ValueKind::Map(Fields::detached(vec![scalar(
                "port",
                Scalar::Int(port),
            )])))
        };
        let fields = Fields::detached(vec![
            Field::detached_value("svc", Value::detached(ValueKind::Seq(vec![map(1), map(2)])))
                .with_doc(Some("A service entry.".to_string())),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(
            text.matches("# A service entry.").count(),
            1,
            "got:\n{text}"
        );
        assert_eq!(text.matches("[[svc]]").count(), 2, "got:\n{text}");
    }

    #[test]
    fn emit_toml_starts_a_doc_commented_first_table_without_a_blank_line() {
        // Arrange
        let fields = Fields::detached(vec![
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            )
            .with_doc(Some("Request limits.".to_string())),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert!(text.starts_with("# Request limits.\n"), "got: {text:?}");
    }

    #[test]
    fn emit_toml_uses_a_later_blocks_doc_when_the_first_has_none() {
        // Arrange
        // Only one comment can render above the grouped array, so the group
        // takes the first doc any element carries.
        let block = |port: i64| {
            Field::detached_block(
                "svc",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![
            block(1),
            block(2).with_doc(Some("A service entry.".to_string())),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(
            text.matches("# A service entry.").count(),
            1,
            "got:\n{text}"
        );
    }

    #[test]
    fn emit_toml_rejects_two_values_sharing_a_name() {
        // Arrange
        // A TOML key names one thing, so a second value under the same name
        // would silently overwrite the first.
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            scalar("x", Scalar::Int(2)),
        ]);

        // Act
        let result = emit_toml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "".to_string(),
            })
        );
    }

    #[test]
    fn emit_toml_rejects_a_repeated_name_inside_an_inline_table() {
        // Arrange
        // Nothing repeats inside an inline table, not even blocks, which have
        // no array-of-tables spelling there.
        let pair = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            scalar("x", Scalar::Int(2)),
        ]);
        let fields = Fields::detached(vec![Field::detached_value(
            "obj",
            Value::detached(ValueKind::Map(pair)),
        )]);

        // Act
        let result = emit_toml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "obj".to_string(),
            })
        );
    }

    #[test]
    fn emit_toml_names_the_nested_path_in_an_error() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_block(
            "limits",
            Fields::detached(vec![Field::detached_value(
                "when",
                Value::detached(ValueKind::Other("datetime")),
            )]),
        )]);

        // Act
        let result = emit_toml(&fields);

        // Assert
        let error = result.unwrap_err();
        assert_eq!(
            error,
            EmitError::UnrepresentableValue {
                label: "datetime",
                path: "limits.when".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "cannot emit datetime: the value has no representation in the model (at `limits.when`)"
        );
    }

    #[test]
    fn emit_toml_round_trips_an_adversarial_string() {
        // Arrange
        // Escaping goes through toml_edit, so this guards the crate against a
        // regression in how quotes, backslashes, line breaks, tabs, unicode,
        // and control characters are spelled.
        let hostile = "quote\" backslash\\ newline\n tab\t snowman\u{2603} del\u{7f} bel\u{7}";
        let fields = Fields::detached(vec![scalar(
            "greeting",
            Scalar::String(hostile.to_string()),
        )]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        let round = reparse(&text);
        let mut report = Report::new();
        let parsed = parse_string_field(round.get("greeting").unwrap(), &mut report).unwrap();
        assert_eq!(parsed.value, hostile, "emitted: {text:?}");
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_toml_quotes_a_key_that_needs_quoting() {
        // Arrange
        // A dotted bare key would read as nesting, so the key must be quoted.
        let fields = Fields::detached(vec![scalar("a.b", Scalar::Int(1))]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert!(text.contains("\"a.b\""), "got: {text:?}");
        let round = reparse(&text);
        let mut report = Report::new();
        let parsed = parse_int_field(round.get("a.b").unwrap(), &mut report).unwrap();
        assert_eq!(parsed.value, 1);
    }

    #[test]
    fn emit_toml_renders_a_doc_with_format_significant_characters() {
        // Arrange
        // `#`, brackets, and quotes are all meaningful TOML syntax, but inside
        // a comment they are plain text and must stay verbatim.
        let doc = "# not a nested comment [not.a.table] \"not a string\"";
        let fields = Fields::detached(vec![
            scalar("port", Scalar::Int(1)).with_doc(Some(doc.to_string())),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert!(text.contains(&format!("# {doc}\n")), "got: {text:?}");
        reparse(&text);
    }

    #[test]
    fn emit_toml_round_trips_a_non_finite_float() {
        // TOML spells infinity and NaN as keywords, so these emit rather than
        // fail, unlike the HCL emitter which has no literal for them.
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            // Arrange
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(value))]);

            // Act
            let text = emit_toml(&fields).unwrap();

            // Assert
            let round = reparse(&text);
            let mut report = Report::new();
            let parsed = parse_float_field(round.get("rate").unwrap(), &mut report).unwrap();
            if value.is_nan() {
                assert!(parsed.value.is_nan(), "emitted: {text:?}");
            } else {
                assert_eq!(parsed.value, value, "emitted: {text:?}");
            }
            assert!(!report.has_issues());
        }
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
            Err(EmitError::UnrepresentableValue {
                label: "datetime",
                path: "when".to_string(),
            })
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
        // Arrange
        // A NUL is not a printable TOML comment character.
        let fields = Fields::detached(vec![
            scalar("port", Scalar::Int(1)).with_doc(Some("a\0b".to_string())),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        // Emit drops the NUL, so the template reparses.
        assert!(text.contains("# ab\n"), "got: {text:?}");
        reparse(&text);
    }
}
