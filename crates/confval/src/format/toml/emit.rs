//! TOML write path: serializes a neutral [`Fields`] tree to canonical TOML.
//!
//! This is the inverse of [`parse_toml_fields`](super::parse_toml_fields). It
//! builds a `toml_edit` document by structure and renders it, filling in the
//! doc comments an annotated template has.

use super::commented::{child_header, commented_block_text, commented_value_text};
use crate::format::EmitError;
use crate::format::emit::{
    blocks_named, child_path, comment_lines, first_conflicting_name, refuse_label, repeated_name,
    values_then_blocks,
};
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
    let pending = emit_table(fields, document.as_table_mut(), "", "")?;
    if !pending.is_empty() {
        document.set_trailing(pending);
    }
    let text = document.to_string();
    // A doc-commented section has a leading blank line to separate it from
    // what precedes it. At the top of the document nothing precedes it.
    match text.strip_prefix('\n') {
        Some(stripped) => Ok(stripped.to_string()),
        None => Ok(text),
    }
}

/// A name whose active uses at one level TOML cannot write side by side.
/// Repeated blocks group into an array of tables, so a name repeated only by
/// blocks is fine. Any other repetition, two values or a value next to a
/// block, would silently overwrite one of them. Emit refuses instead. A
/// commented field is comment text, so it conflicts with nothing.
fn conflicting_name(fields: &Fields) -> Option<&str> {
    first_conflicting_name(fields, |group| {
        group.len() > 1
            && !group
                .iter()
                .all(|field| matches!(field.kind, FieldKind::Block(_)))
    })
}

/// Fills a `toml_edit` table from a neutral level.
///
/// `header` is the quoted dotted header of this level, empty at the root, so a
/// commented entry can write the `#[header.key]` line of a nested block.
/// Returns the commented-out text still pending at the level's end. TOML has
/// no per-table trailing position, so the caller attaches it before the next
/// structure in document order. Text pending at the top attaches to the
/// document's trailing slot. TOML's grammar reads both positions as belonging
/// to the earlier table.
fn emit_table(
    fields: &Fields,
    table: &mut Table,
    path: &str,
    header: &str,
) -> Result<String, EmitError> {
    if let Some(error) = refuse_label(fields, path) {
        return Err(error);
    }
    if let Some(name) = conflicting_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let mut pending = String::new();
    let mut grouped: HashSet<&str> = HashSet::new();
    // toml_edit renders every value of a table above its subtables, whatever
    // the insertion order, so commented text walks the same values-then-blocks
    // partition. A commented entry then lands in the region its active twin
    // would render in, and uncommenting cannot bind a value into the wrong
    // table.
    for entry in values_then_blocks(fields) {
        let field = entry.field();
        match &field.kind {
            FieldKind::Value(value) => {
                let child = child_path(path, &field.name);
                if entry.is_commented() {
                    pending.push_str(&commented_value_text(field, value, &child, header)?);
                    continue;
                }
                let (mut item, inner_pending) =
                    item_of_value(value, &child, &child_header(header, &field.name))?;
                // An array of tables renders its key once per `[[element]]`,
                // so a comment on the key would repeat. It goes above the
                // first element instead, like the block path's grouped form.
                if let Item::ArrayOfTables(array) = &mut item {
                    let prefix = block_prefix(&mut pending, field.doc.as_deref());
                    if let (Some(prefix), Some(first)) = (prefix, array.iter_mut().next()) {
                        first.decor_mut().set_prefix(prefix);
                    }
                    table.insert(&field.name, item);
                } else {
                    table.insert(&field.name, item);
                    let prefix = value_prefix(&mut pending, field.doc.as_deref());
                    if let (Some(prefix), Some(mut entry)) =
                        (prefix, table.get_key_value_mut(&field.name))
                    {
                        entry.0.leaf_decor_mut().set_prefix(prefix);
                    }
                }
                pending.push_str(&inner_pending);
            }
            FieldKind::Block(inner) => {
                let child = child_path(path, &field.name);
                if entry.is_commented() {
                    pending.push_str(&commented_block_text(field, inner, &child, header)?);
                    continue;
                }
                // Group same-named blocks by name across the level, so a
                // non-consecutive repeat is kept rather than overwritten.
                if !grouped.insert(field.name.as_str()) {
                    continue;
                }
                let blocks = blocks_named(fields, &field.name);
                // Only one comment can render above the group, so the group
                // takes the first doc any element has.
                let doc = fields
                    .iter()
                    .filter(|other| other.name == field.name)
                    .find_map(|other| other.doc.as_deref());
                let sub_header = child_header(header, &field.name);
                if blocks.len() == 1 {
                    let mut subtable = Table::new();
                    let sub_pending = emit_table(blocks[0], &mut subtable, &child, &sub_header)?;
                    if let Some(prefix) = block_prefix(&mut pending, doc) {
                        subtable.decor_mut().set_prefix(prefix);
                    }
                    table.insert(&field.name, Item::Table(subtable));
                    pending = sub_pending;
                } else {
                    // The comment renders once, above the first array-of-tables
                    // element. Each element's pending text attaches before
                    // the next element's header.
                    let mut array = ArrayOfTables::new();
                    for (index, inner) in blocks.into_iter().enumerate() {
                        let mut subtable = Table::new();
                        let sub_pending = emit_table(inner, &mut subtable, &child, &sub_header)?;
                        let doc = (index == 0).then_some(doc).flatten();
                        if let Some(prefix) = block_prefix(&mut pending, doc) {
                            subtable.decor_mut().set_prefix(prefix);
                        }
                        array.push(subtable);
                        pending = sub_pending;
                    }
                    table.insert(&field.name, Item::ArrayOfTables(array));
                }
            }
        }
    }
    Ok(pending)
}

/// The decor prefix for an active value: any pending commented text first,
/// then the value's doc comment. `None` leaves the default decor untouched.
fn value_prefix(pending: &mut String, doc: Option<&str>) -> Option<String> {
    let pending = std::mem::take(pending);
    match (pending.is_empty(), doc) {
        (true, None) => None,
        (_, doc) => Some(format!(
            "{pending}{}",
            doc.map(toml_comment).unwrap_or_default()
        )),
    }
}

/// The decor prefix for a `[table]` or `[[array of tables]]` element: any
/// pending commented text first, then the blank line and doc comment the plain
/// dump has. `None` leaves toml_edit's default header spacing untouched.
fn block_prefix(pending: &mut String, doc: Option<&str>) -> Option<String> {
    let pending = std::mem::take(pending);
    match (pending.is_empty(), doc) {
        (true, None) => None,
        (_, doc) => Some(format!(
            "{pending}\n{}",
            doc.map(toml_comment).unwrap_or_default()
        )),
    }
}

/// Maps one neutral value to a table item: a scalar, an inline array, an inline
/// table, or a `[[array of tables]]` for a non-empty sequence of maps.
pub(super) fn item_of_value(
    value: &Value,
    path: &str,
    header: &str,
) -> Result<(Item, String), EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => Ok((Item::Value(toml_value_of_scalar(scalar)), String::new())),
        ValueKind::Seq(elements) => {
            if !elements.is_empty()
                && elements
                    .iter()
                    .all(|element| matches!(element.kind, ValueKind::Map(_)))
            {
                let mut array = ArrayOfTables::new();
                let mut pending = String::new();
                for element in elements {
                    if let ValueKind::Map(inner) = &element.kind {
                        let mut subtable = Table::new();
                        let sub_pending = emit_table(inner, &mut subtable, path, header)?;
                        if !pending.is_empty() {
                            subtable
                                .decor_mut()
                                .set_prefix(format!("{}\n", std::mem::take(&mut pending)));
                        }
                        array.push(subtable);
                        pending = sub_pending;
                    }
                }
                Ok((Item::ArrayOfTables(array), pending))
            } else {
                Ok((
                    Item::Value(TomlValue::Array(toml_array_of(elements, path)?)),
                    String::new(),
                ))
            }
        }
        ValueKind::Map(inner) => Ok((
            Item::Value(TomlValue::InlineTable(toml_inline_of(inner, path)?)),
            String::new(),
        )),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// Maps one neutral value to an inline `toml_edit` value, for an array element
/// or an inline-table entry. A sequence of maps is an inline array of inline
/// tables here, because an array of tables has no inline form.
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
    if let Some(error) = refuse_label(fields, path) {
        return Err(error);
    }
    if let Some(name) = repeated_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let mut inline = InlineTable::new();
    // An inline table has no comment syntax, and `iter` yields no commented
    // entry, so one renders nothing here.
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
/// comment has no indentation.
pub(super) fn toml_comment(doc: &str) -> String {
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
    fn emit_toml_rejects_a_native_label_it_cannot_write() {
        // Arrange
        // A parsed HCL or KDL block keeps its label on the inner level, and
        // TOML has no label syntax and no field name to write it with.
        let inner = Fields::detached(vec![scalar("host", Scalar::String("h".to_string()))])
            .with_label(Located::detached("api".to_string()));
        let fields = Fields::detached(vec![Field::detached_block("upstream", inner)]);

        // Act
        let result = emit_toml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableLabel {
                label: "api".to_string(),
                path: "upstream".to_string(),
            })
        );
    }

    #[test]
    fn emit_toml_rejects_a_value_and_a_block_sharing_a_name() {
        // Arrange
        // HCL writes `x = 1` next to `x { }`, so a parsed Fields can hold
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
    fn emit_toml_writes_an_inline_table_with_several_distinct_keys() {
        // Arrange
        // An inline table admits no repetition, so the scan guarding it
        // must still accept a level whose keys are distinct. A nested
        // inline table covers the recursion.
        let inner = Fields::detached(vec![
            scalar("ca", Scalar::String("ca.pem".to_string())),
            scalar("verify", Scalar::Bool(true)),
        ]);
        let map = Fields::detached(vec![
            scalar("cert", Scalar::String("a.pem".to_string())),
            scalar("key", Scalar::String("a.key".to_string())),
            Field::detached_value("trust", Value::detached(ValueKind::Map(inner))),
        ]);
        let fields = Fields::detached(vec![Field::detached_value(
            "tls",
            Value::detached(ValueKind::Seq(vec![Value::detached(ValueKind::Map(map))])),
        )]);

        // Act
        let text = emit_toml(&fields).expect("distinct keys are not a conflict");

        // Assert
        let round = reparse(&text);
        let FieldKind::Value(value) = &round.get("tls").unwrap().kind else {
            panic!("tls should be an attribute");
        };
        let ValueKind::Seq(elements) = &value.kind else {
            panic!("tls should be a sequence");
        };
        let ValueKind::Map(table) = &elements[0].kind else {
            panic!("the element should be a table");
        };
        let names: Vec<&str> = table.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["cert", "key", "trust"]);
        assert!(table.get("trust").is_some());
    }

    #[test]
    fn emit_toml_writes_a_repeated_block_doc_above_the_first_element() {
        // Arrange
        // Two same-named block fields group into an array of tables, and the
        // group's doc renders once, above the first element. This is the block
        // path rather than the sequence-of-maps path, which reaches the same
        // form through a value field.
        let svc = |port: i64| {
            Field::detached_block(
                "svc",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![
            svc(1).with_doc(Some("A service entry.".to_string())),
            svc(2),
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
        let doc_at = text.find("# A service entry.").expect("the doc renders");
        let first_header = text.find("[[svc]]").expect("the first header renders");
        assert!(
            doc_at < first_header,
            "the doc belongs above the first element, got:\n{text}"
        );
        reparse(&text);
    }

    #[test]
    fn emit_toml_attaches_a_pending_comment_above_the_next_array_element() {
        // Arrange
        // A commented-out entry inside an element renders as pending text that
        // belongs above the following `[[element]]` header. With one element
        // there is no following header, so the placement needs two.
        let element = |port: i64| {
            Value::detached(ValueKind::Map(Fields::detached_entries(vec![
                scalar("port", Scalar::Int(port)).into(),
                scalar("pid_file", Scalar::String(String::new()))
                    .with_doc(Some("The PID file path.".to_string()))
                    .as_commented(),
            ])))
        };
        let fields = Fields::detached(vec![Field::detached_value(
            "svc",
            Value::detached(ValueKind::Seq(vec![element(1), element(2)])),
        )]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        let commented = text.find("#pid_file").expect("the commented entry renders");
        let first_header = text.find("[[svc]]").expect("the first header renders");
        let second_header = text[first_header + 1..]
            .find("[[svc]]")
            .expect("the second header renders")
            + first_header
            + 1;
        assert!(
            first_header < commented && commented < second_header,
            "the first element's commented entry sits between the two headers, got:\n{text}"
        );
        reparse(&text);
    }

    #[test]
    fn emit_toml_orders_a_commented_level_values_before_blocks() {
        // Arrange
        // A commented block has its own level, and that level walks
        // values before blocks the way an active one does.
        let inner = Fields::detached(vec![
            Field::detached_block(
                "retry",
                Fields::detached(vec![scalar("attempts", Scalar::Int(3))]),
            ),
            scalar("mode", Scalar::String("log".to_string())),
        ]);
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            Field::detached_block("limits", inner).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        let value_line = text.find("#mode").expect("the commented value renders");
        let block_line = text
            .find("#[limits.retry]")
            .expect("the commented block header renders");
        assert!(
            value_line < block_line,
            "a commented level orders values before blocks, got:\n{text}"
        );
        assert!(text.contains("#attempts = 3"), "got:\n{text}");
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
        // Counting the comment does not say which element it appears above, and
        // rendering it above the last would keep the count at one.
        let doc_at = text.find("# A service entry.").expect("the doc renders");
        let first_header = text.find("[[svc]]").expect("the first header renders");
        let second_header = text[first_header + 1..]
            .find("[[svc]]")
            .expect("the second header renders")
            + first_header
            + 1;
        assert!(doc_at < first_header, "the doc precedes the first element");
        assert!(first_header < second_header);
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
        // takes the first doc any element has.
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
        // no array-of-tables form there.
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
        // and control characters are escaped.
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
        // TOML writes infinity and NaN as keywords, so these emit rather than
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
    fn emit_toml_writes_a_commented_leaf_after_the_active_values() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n#pid_file = \"\"\n");
    }

    #[test]
    fn emit_toml_writes_a_commented_leaf_before_an_active_value() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
            scalar("port", Scalar::Int(8080)).into(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "#pid_file = \"\"\nport = 8080\n");
    }

    #[test]
    fn emit_toml_renders_a_doc_above_its_commented_entry() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("pid_file", Scalar::String(String::new()))
                .with_doc(Some("The PID file path.".to_string()))
                .as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "port = 8080\n# The PID file path.\n#pid_file = \"\"\n"
        );
    }

    #[test]
    fn emit_toml_writes_a_commented_empty_block_as_a_commented_header() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n\n#[tls]\n");
    }

    #[test]
    fn emit_toml_writes_a_commented_list_hint_as_an_array_of_tables_header() {
        // Arrange
        // The nested-list shape, a sequence of one empty map, writes the
        // repetition where a single block would not.
        let hint = Value::detached(ValueKind::Seq(vec![Value::detached(ValueKind::Map(
            Fields::detached(vec![]),
        ))]));
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            Field::detached_value("svc", hint).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n\n#[[svc]]\n");
    }

    #[test]
    fn emit_toml_attaches_a_commented_entry_above_a_doc_commented_table() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            )
            .with_doc(Some("Request limits.".to_string()))
            .into(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        // The commented entry renders first, then the blank line and doc the
        // table has.
        assert_eq!(
            text,
            "#pid_file = \"\"\n\n# Request limits.\n[limits]\nmax_body_mb = 16\n"
        );
    }

    #[test]
    fn emit_toml_renders_an_all_commented_table_after_its_header() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_block(
            "limits",
            Fields::detached_entries(vec![scalar("max_body_mb", Scalar::Int(16)).as_commented()]),
        )]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "[limits]\n#max_body_mb = 16\n");
    }

    #[test]
    fn emit_toml_renders_a_commented_block_after_the_values_it_precedes() {
        // Arrange
        // toml_edit renders values above tables, so a commented block declared
        // first must still land in the block region. An uncommented `[tls]`
        // above `port` would capture the value into the wrong table.
        let fields = Fields::detached_entries(vec![
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
            scalar("port", Scalar::Int(8080)).into(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n\n#[tls]\n");
    }

    #[test]
    fn emit_toml_renders_a_commented_value_in_the_value_region() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            )
            .into(),
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "#pid_file = \"\"\n\n[limits]\nmax_body_mb = 16\n");
    }

    #[test]
    fn emit_toml_prefixes_every_line_of_a_commented_multiline_string() {
        // Arrange
        // TOML writes a string with line breaks as a multiline literal, and a
        // bare continuation line would break the template's own reparse.
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("motd", Scalar::String("a\nb".to_string())).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n#motd = \"\"\"\n#a\n#b\"\"\"\n");
        reparse(&text);
    }

    #[test]
    fn emit_toml_renders_adjacent_commented_entries_in_order() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("a", Scalar::Int(1)).as_commented(),
            scalar("b", Scalar::Int(2)).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n#a = 1\n#b = 2\n");
    }

    #[test]
    fn emit_toml_drops_a_commented_field_inside_an_inline_table() {
        // Arrange
        // An inline table has no comment syntax, and the field reads as
        // absent.
        let map = Fields::detached_entries(vec![
            scalar("cert", Scalar::String("a.pem".to_string())).into(),
            scalar("key", Scalar::String(String::new())).as_commented(),
        ]);
        let fields = Fields::detached(vec![Field::detached_value(
            "tls",
            Value::detached(ValueKind::Map(map)),
        )]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "tls = { cert = \"a.pem\" }\n");
    }

    #[test]
    fn emit_toml_excludes_commented_fields_from_the_conflict_checks() {
        // Arrange
        // The commented placeholder must never block an active field's
        // emission.
        let fields = Fields::detached_entries(vec![
            scalar("x", Scalar::Int(1)).into(),
            scalar("x", Scalar::Int(2)).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        assert_eq!(text, "x = 1\n#x = 2\n");
    }

    #[test]
    fn emit_toml_reparses_a_commented_template_to_the_active_fields_alone() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
        ]);

        // Act
        let text = emit_toml(&fields).unwrap();

        // Assert
        let round = reparse(&text);
        let names: Vec<&str> = round.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["port"]);
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
