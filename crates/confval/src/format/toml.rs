//! TOML frontend: parses TOML text into the format-neutral [`Fields`] tree.
//!
//! Like [`hcl`](crate::format::hcl), this module's only job is the conversion
//! from a concrete syntax tree (here `toml_edit`'s) to the owned model in
//! [`field`](crate::format::field). It parses through
//! [`Document`], the immutable document type that
//! retains source spans, and emits the same neutral `Fields` every other
//! frontend does, so the leaf parsers, the derive-generated walks, and the
//! hand-written [`FromFields`] impls work against it unchanged.
//!
//! TOML's structural shapes map onto the neutral model as follows:
//!
//! - A `[table]` section becomes a [`FieldKind::Block`], mirroring an HCL
//!   block.
//! - An inline table (`x = { ... }`) becomes a [`FieldKind::Value`] holding a
//!   [`ValueKind::Map`], mirroring an HCL object attribute.
//! - An array of tables (`[[x]]`) becomes one field whose value is a
//!   [`ValueKind::Seq`] of maps, so a `Vec<Located<S>>` nested-list field
//!   lowers from it exactly as it would from an HCL array of objects.
//! - A native datetime, which the neutral model has no scalar for, becomes
//!   [`ValueKind::Other`] and surfaces as an ordinary type mismatch.

use crate::diagnostic::Report;
use crate::format::EmitError;
use crate::format::field::{Field, FieldKind, Fields, FromFields, Scalar, Value, ValueKind};
use crate::source::{SourceId, SourceMap, Span};
use std::collections::HashSet;
use std::ops::Range;
use toml_edit::{
    Array, ArrayOfTables, Document, DocumentMut, InlineTable, Item, Table, Value as TomlValue,
};

/// Parses one registered source into the neutral [`Fields`] tree.
///
/// When you assemble configuration from several sources, you hold the returned
/// `Fields`, merge it with the others, and run [`FromFields`] once on the
/// merged result. A syntax error, the only failure that yields no tree, is
/// reported and returns `None`. Field-level problems are reported but do not
/// stop the parse, so a tree that parsed still reaches validation.
pub fn parse_toml_fields(sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
    let Some(source) = sources.get(id) else {
        report
            .error("internal error: parse_toml_fields called with an unregistered source id")
            .emit();
        return None;
    };
    match Document::parse(&source.text) {
        Ok(document) => {
            let enclosing = Span::new(id, 0, source.text.len() as u32);
            Some(fields_of_table(document.as_table(), enclosing, id, report))
        }
        Err(error) => {
            report
                .error(format!("syntax error: {}", error.message()))
                .at(span_of(error.span(), id))
                .emit();
            None
        }
    }
}

/// Parses one registered source into a `T`, pushing syntax errors and
/// structural problems into the report.
pub fn parse_toml<T: FromFields>(
    sources: &SourceMap,
    id: SourceId,
    report: &mut Report,
) -> Option<T> {
    let fields = parse_toml_fields(sources, id, report)?;
    T::from_fields(&fields, report)
}

fn span_of(range: Option<Range<usize>>, source: SourceId) -> Span {
    match range {
        Some(range) => Span::new(source, range.start as u32, range.end as u32),
        None => Span::detached(),
    }
}

/// The whole-field span, name and value together. Either end may be missing
/// (an absent toml_edit span). The present one then stands alone.
fn entry_span(name_span: Span, value_span: Span) -> Span {
    if name_span.is_detached() {
        value_span
    } else if value_span.is_detached() {
        name_span
    } else {
        Span::merge(name_span, value_span)
    }
}

/// Normalizes a table's entries into neutral fields. Used for the document
/// root, for `[section]` tables, and for each `[[array]]` element.
fn fields_of_table(
    table: &Table,
    enclosing: Span,
    source: SourceId,
    report: &mut Report,
) -> Fields {
    let mut items = Vec::new();
    for (name, item) in table.iter() {
        let name_span = span_of(
            table.get_key_value(name).and_then(|(key, _)| key.span()),
            source,
        );
        items.push(field_of_item(name, name_span, item, source, report));
    }
    Fields::new(source, enclosing, items)
}

/// Builds one field from a table entry, classifying it as a block (section),
/// an array of tables, or an attribute value.
fn field_of_item(
    name: &str,
    name_span: Span,
    item: &Item,
    source: SourceId,
    report: &mut Report,
) -> Field {
    let value_span = span_of(item.span(), source);
    let kind = if let Some(table) = item.as_table() {
        FieldKind::Block(fields_of_table(table, value_span, source, report))
    } else if let Some(array) = item.as_array_of_tables() {
        let elements = array
            .iter()
            .map(|table| {
                let span = span_of(table.span(), source);
                Value {
                    span,
                    kind: ValueKind::Map(fields_of_table(table, span, source, report)),
                }
            })
            .collect();
        FieldKind::Value(Value {
            span: value_span,
            kind: ValueKind::Seq(elements),
        })
    } else if let Some(value) = item.as_value() {
        FieldKind::Value(value_of_value(value, source, report))
    } else {
        FieldKind::Value(Value {
            span: value_span,
            kind: ValueKind::Other("value"),
        })
    };
    Field {
        name: name.to_string(),
        name_span,
        span: entry_span(name_span, value_span),
        source,
        kind,
    }
}

/// Converts one TOML value into a neutral [`Value`], recursing through arrays
/// and inline tables. A datetime has no neutral scalar and becomes
/// [`ValueKind::Other`].
fn value_of_value(value: &TomlValue, source: SourceId, report: &mut Report) -> Value {
    let span = span_of(value.span(), source);
    let kind = if let Some(string) = value.as_str() {
        ValueKind::Scalar(Scalar::String(string.to_string()))
    } else if let Some(boolean) = value.as_bool() {
        ValueKind::Scalar(Scalar::Bool(boolean))
    } else if let Some(int) = value.as_integer() {
        ValueKind::Scalar(Scalar::Int(int))
    } else if let Some(float) = value.as_float() {
        ValueKind::Scalar(Scalar::Float(float))
    } else if let Some(array) = value.as_array() {
        ValueKind::Seq(
            array
                .iter()
                .map(|element| value_of_value(element, source, report))
                .collect(),
        )
    } else if let Some(inline) = value.as_inline_table() {
        ValueKind::Map(fields_of_inline_table(inline, span, source, report))
    } else if value.is_datetime() {
        ValueKind::Other("datetime")
    } else {
        ValueKind::Other("value")
    };
    Value { span, kind }
}

/// Normalizes an inline table's entries into neutral fields. An inline table
/// holds only values, so every entry is a [`FieldKind::Value`].
fn fields_of_inline_table(
    table: &InlineTable,
    enclosing: Span,
    source: SourceId,
    report: &mut Report,
) -> Fields {
    let mut items = Vec::new();
    for (name, value) in table.iter() {
        let name_span = span_of(
            table.get_key_value(name).and_then(|(key, _)| key.span()),
            source,
        );
        let value = value_of_value(value, source, report);
        let span = entry_span(name_span, value.span);
        items.push(Field {
            name: name.to_string(),
            name_span,
            span,
            source,
            kind: FieldKind::Value(value),
        });
    }
    Fields::new(source, enclosing, items)
}

/// Serializes a [`Fields`] tree to canonical TOML text.
///
/// This is the inverse of [`parse_toml_fields`]. It builds a `toml_edit`
/// document by structure and returns its text, dropping the comments and layout
/// the neutral model never held. Same-named blocks at one level group into a
/// `[[array of tables]]`, so a repeated block keeps every element rather than
/// overwriting a sibling. It fails only on a [`ValueKind::Other`], which
/// populate never produces.
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
                    table.insert(&field.name, Item::Table(subtable));
                } else {
                    let mut array = ArrayOfTables::new();
                    for inner in blocks {
                        let mut subtable = Table::new();
                        emit_table(inner, &mut subtable)?;
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
    use super::*;
    use crate::format::field::{
        parse_bool_field, parse_float_field, parse_int_field, parse_string_field,
        parse_string_list_field, parse_struct_field, parse_struct_list_field, report_unknown_field,
    };
    use crate::source::Located;

    struct Probe;
    impl FromFields for Probe {
        fn from_fields(_: &Fields, _: &mut Report) -> Option<Self> {
            Some(Probe)
        }
    }

    fn parse(input: &str) -> (SourceMap, SourceId, Fields) {
        let mut sources = SourceMap::new();
        let id = sources.add("test.toml", input);
        let document = Document::parse(sources.get(id).unwrap().text.clone()).unwrap();
        let mut report = Report::new();
        let fields = fields_of_table(document.as_table(), Span::new(id, 0, 0), id, &mut report);
        assert!(
            !report.has_issues(),
            "frontend reported: {:?}",
            report.issues()
        );
        (sources, id, fields)
    }

    #[test]
    fn string_field_parses_with_value_span() {
        let input = "name = \"api\"\n";
        let (_, _, fields) = parse(input);
        let mut report = Report::new();
        let value = parse_string_field(fields.get("name").unwrap(), &mut report).unwrap();
        assert_eq!(value.value, "api");
        assert_eq!(
            &input[value.span.start as usize..value.span.end as usize],
            "\"api\""
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn int_and_float_are_distinguished() {
        let (_, _, fields) = parse("port = 8080\nratio = 1.5\n");
        let mut report = Report::new();
        assert_eq!(
            parse_int_field(fields.get("port").unwrap(), &mut report)
                .unwrap()
                .value,
            8080
        );
        // A TOML float is not an integer, so the int parser rejects it.
        assert!(parse_int_field(fields.get("ratio").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected integer, found number");
        assert_eq!(
            parse_float_field(fields.get("ratio").unwrap(), &mut report)
                .unwrap()
                .value,
            1.5
        );
    }

    #[test]
    fn bool_field_parses() {
        let (_, _, fields) = parse("daemon = true\n");
        let mut report = Report::new();
        assert!(
            parse_bool_field(fields.get("daemon").unwrap(), &mut report)
                .unwrap()
                .value
        );
    }

    #[test]
    fn string_list_has_per_element_spans() {
        let input = "allow = [\"10.0.0.0/8\", \"192.168.0.0/16\"]\n";
        let (_, _, fields) = parse(input);
        let mut report = Report::new();
        let list = parse_string_list_field(fields.get("allow").unwrap(), &mut report).unwrap();
        assert_eq!(list.value.len(), 2);
        let first = &list.value[0];
        assert_eq!(first.value, "10.0.0.0/8");
        assert_eq!(
            &input[first.span.start as usize..first.span.end as usize],
            "\"10.0.0.0/8\""
        );
    }

    #[test]
    fn section_parses_as_block() {
        let input = "[tls]\ncert = \"a.pem\"\n";
        let (_, _, fields) = parse(input);
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        assert!(parsed.is_some());
        assert!(!report.has_issues());
    }

    #[test]
    fn inline_table_parses_as_object() {
        let (_, _, fields) = parse("tls = { cert = \"a.pem\" }\n");
        let mut report = Report::new();
        let FieldKind::Value(value) = &fields.get("tls").unwrap().kind else {
            panic!("expected attribute value");
        };
        let ValueKind::Map(inner) = &value.kind else {
            panic!("expected inline table to become a map");
        };
        assert!(inner.get("cert").is_some());
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        assert!(parsed.is_some());
    }

    #[test]
    fn array_of_tables_lowers_as_nested_list() {
        let input = "[[upstream]]\nendpoint = \"10.0.0.1:9000\"\n[[upstream]]\nendpoint = \"10.0.0.2:9000\"\n";
        let (_, _, fields) = parse(input);
        let mut report = Report::new();
        let mut upstreams: Vec<Located<Probe>> = Vec::new();
        parse_struct_list_field(&mut upstreams, fields.get("upstream").unwrap(), &mut report);
        assert_eq!(upstreams.len(), 2);
        assert!(!report.has_issues());
    }

    #[test]
    fn datetime_becomes_other_and_mismatches() {
        // A native TOML datetime has no neutral scalar, so it must surface as a
        // type mismatch, not silently parse.
        let (_, _, fields) = parse("when = 1979-05-27T07:32:00Z\n");
        let mut report = Report::new();
        assert!(parse_string_field(fields.get("when").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected string, found datetime"
        );
    }

    #[test]
    fn unknown_field_reported_at_name_span() {
        let (_, id, fields) = parse("hostnme = \"typo\"\n");
        let mut report = Report::new();
        report_unknown_field(fields.get("hostnme").unwrap(), &mut report);
        assert_eq!(report.issues()[0].message, "unknown field: hostnme");
        assert_eq!(report.issues()[0].span, Some(Span::new(id, 0, 7)));
    }

    #[test]
    fn syntax_error_is_reported_with_location() {
        let mut sources = SourceMap::new();
        let id = sources.add("broken.toml", "port = \n");
        let mut report = Report::new();
        let parsed: Option<Probe> = parse_toml(&sources, id, &mut report);
        assert!(parsed.is_none());
        assert!(report.has_errors());
        assert!(report.issues()[0].message.starts_with("syntax error:"));
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
}
