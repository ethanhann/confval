//! JSON write path: serializes a neutral [`Fields`] tree to canonical JSON.
//!
//! This is the inverse of [`parse_json_fields`](super::parse_json_fields). It
//! writes the text directly, because JSON's layout is small enough to state in
//! one place: two-space indentation, one member per line, no blank lines, and a
//! trailing newline.

use crate::format::EmitError;
use crate::format::emit::{
    blocks_named, child_path, grouped_elements, indent, refuse_label, value_beside_block,
    values_named, values_then_blocks,
};
use crate::format::field::{FieldKind, Fields, Scalar, Value, ValueKind};

/// Serializes a [`Fields`] tree to canonical JSON text.
///
/// This is the inverse of [`parse_json_fields`](super::parse_json_fields). It
/// writes a pretty-printed object and returns its text, dropping the layout the
/// neutral model never held. Values emit before blocks at each level, each
/// group in declaration order, with two-space indentation and a trailing
/// newline. An array of scalars stays on one line, and an array holding an
/// object or an array takes one line per element.
///
/// JSON has no comment syntax, so doc comments are dropped and commented
/// entries are skipped. Emitting an annotated template therefore produces the
/// same text as emitting the populated model.
///
/// Same-named fields at one level group into one member holding an array, so a
/// tree parsed from duplicate keys still emits. Grouping collapses several
/// fields into one member, so the round trip over duplicates holds at the
/// walk's resolution rather than at the `Fields` level.
///
/// It fails on a [`ValueKind::Other`], on a non-finite float, and on a value
/// beside a same-named block. JSON has no literal for a non-finite float. Its
/// only way to write the value-beside-block pair is a duplicate key, which most
/// consumers collapse to one member. Every name is representable, because any
/// key writes as a JSON string, so [`EmitError::UnrepresentableName`] never
/// arises. Emit of a populated spec fails only for a non-finite float default.
pub fn emit_json(fields: &Fields) -> Result<String, EmitError> {
    let mut out = String::new();
    write_object(&mut out, fields, 0, "")?;
    out.push('\n');
    Ok(out)
}

/// One member of an emitted object: the same-named fields sharing its name, in
/// declaration order.
enum Member<'a> {
    Values(Vec<&'a Value>),
    Blocks(Vec<&'a Fields>),
}

/// The members of one level, values before blocks, each group at its first
/// occurrence's position.
fn members_of(fields: &Fields) -> Vec<(&str, Member<'_>)> {
    let mut members: Vec<(&str, Member)> = Vec::new();
    let mut grouped: Vec<&str> = Vec::new();
    for entry in values_then_blocks(fields) {
        let field = entry.field();
        if entry.is_commented() || grouped.contains(&field.name.as_str()) {
            continue;
        }
        grouped.push(&field.name);
        let member = match field.kind {
            FieldKind::Value(_) => Member::Values(values_named(fields, &field.name)),
            FieldKind::Block(_) => Member::Blocks(blocks_named(fields, &field.name)),
        };
        members.push((&field.name, member));
    }
    members
}

/// Writes one level as an object. `level` is the nesting depth of the line the
/// opening brace is on, so the closing brace lines up with it.
fn write_object(
    out: &mut String,
    fields: &Fields,
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    if let Some(error) = refuse_label(fields, path) {
        return Err(error);
    }
    if let Some(name) = value_beside_block(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let members = members_of(fields);
    if members.is_empty() {
        out.push_str("{}");
        return Ok(());
    }
    out.push_str("{\n");
    for (index, (name, member)) in members.iter().enumerate() {
        indent(out, level + 1);
        write_string(out, name);
        out.push_str(": ");
        write_member(out, member, level + 1, &child_path(path, name))?;
        separate(out, index, members.len());
    }
    indent(out, level);
    out.push('}');
    Ok(())
}

/// Writes one member's value. A lone field writes its own shape. Several
/// same-named fields write one array, because JSON has no second way to write a
/// repeated name.
fn write_member(
    out: &mut String,
    member: &Member,
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    match member {
        Member::Values(group) => match group.as_slice() {
            [only] => write_value(out, only, level, path),
            _ => write_array(out, &grouped_elements(group), level, path),
        },
        Member::Blocks(group) => match group.as_slice() {
            [only] => write_object(out, only, level, path),
            _ => write_objects(out, group, level, path),
        },
    }
}

/// Writes one value by its shape.
fn write_value(out: &mut String, value: &Value, level: usize, path: &str) -> Result<(), EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => write_scalar(out, scalar, path),
        ValueKind::Seq(elements) => {
            let borrowed: Vec<&Value> = elements.iter().collect();
            write_array(out, &borrowed, level, path)
        }
        ValueKind::Map(inner) => write_object(out, inner, level, path),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// Writes an array. An array of scalars stays on one line, and one holding an
/// object or an array takes one line per element.
fn write_array(
    out: &mut String,
    elements: &[&Value],
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    if elements.is_empty() {
        out.push_str("[]");
        return Ok(());
    }
    let structural = elements
        .iter()
        .any(|element| matches!(element.kind, ValueKind::Map(_) | ValueKind::Seq(_)));
    if !structural {
        out.push('[');
        for (index, element) in elements.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            write_value(out, element, level, path)?;
        }
        out.push(']');
        return Ok(());
    }
    out.push_str("[\n");
    for (index, element) in elements.iter().enumerate() {
        indent(out, level + 1);
        write_value(out, element, level + 1, path)?;
        separate(out, index, elements.len());
    }
    indent(out, level);
    out.push(']');
    Ok(())
}

/// Writes repeated same-named blocks as one array of objects, the shape the
/// TOML emitter's `[[name]]` grouping established.
fn write_objects(
    out: &mut String,
    blocks: &[&Fields],
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    out.push_str("[\n");
    for (index, block) in blocks.iter().enumerate() {
        indent(out, level + 1);
        write_object(out, block, level + 1, path)?;
        separate(out, index, blocks.len());
    }
    indent(out, level);
    out.push(']');
    Ok(())
}

/// Writes one scalar literal.
fn write_scalar(out: &mut String, scalar: &Scalar, path: &str) -> Result<(), EmitError> {
    match scalar {
        // An unparsed literal reached the model as text from an environment
        // variable or a flag, so it emits as the string it always was.
        Scalar::String(text) | Scalar::Unparsed(text) => write_string(out, text),
        Scalar::Int(int) => out.push_str(&int.to_string()),
        Scalar::Bool(boolean) => out.push_str(if *boolean { "true" } else { "false" }),
        Scalar::Float(float) => {
            if !float.is_finite() {
                return Err(EmitError::UnrepresentableValue {
                    label: "non-finite float",
                    path: path.to_string(),
                });
            }
            out.push_str(&float_text(*float));
        }
    }
    Ok(())
}

/// A finite float's shortest text in a form that reparses as a float.
///
/// The `Debug` formatting of an `f64` always writes a fraction or an exponent,
/// `100.0` rather than `100` and `1e20` rather than its digit string, so the
/// text never classifies as an integer on reparse. The float form tests
/// pin that property against the parse mapping.
fn float_text(float: f64) -> String {
    format!("{float:?}")
}

/// Writes a JSON string literal, escaping per RFC 8259: the quote, the
/// backslash, and every control character, with the short escapes where they
/// exist. Everything else emits as raw UTF-8, which JSON permits, so non-ASCII
/// text stays readable.
///
/// This body is a coincidental duplicate of `yaml::text::write_string`, not a
/// shared source. JSON escapes per RFC 8259, and YAML's double-quoted repertoire
/// is broader, so the two are free to diverge.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if control < '\u{20}' => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Ends a line inside an object or a multi-line array, with the comma every
/// element but the last one needs.
fn separate(out: &mut String, index: usize, total: usize) {
    if index + 1 < total {
        out.push(',');
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::super::parse_json_fields;
    use super::*;
    use crate::diagnostic::Report;
    use crate::format::field::Field;
    use crate::format::parse::{parse_float_field, parse_string_field, parse_string_list_field};
    use crate::source::SourceMap;

    fn scalar(name: &str, scalar: Scalar) -> Field {
        Field::detached_value(name, Value::detached(ValueKind::Scalar(scalar)))
    }

    fn seq(name: &str, elements: Vec<ValueKind>) -> Field {
        let values = elements.into_iter().map(Value::detached).collect();
        Field::detached_value(name, Value::detached(ValueKind::Seq(values)))
    }

    fn map(fields: Vec<Field>) -> ValueKind {
        ValueKind::Map(Fields::detached(fields))
    }

    fn reparse(text: &str) -> Fields {
        let mut sources = SourceMap::new();
        let id = sources.add("emitted.json", text.to_string());
        let mut report = Report::new();
        let fields = parse_json_fields(&sources, id, &mut report)
            .unwrap_or_else(|| panic!("emitted text should parse: {text}"));
        assert!(
            !report.has_issues(),
            "reparse issues: {:?}",
            report.issues()
        );
        fields
    }

    #[test]
    fn emit_json_writes_canonical_text() {
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
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"hostname\": \"api\",\n  \"port\": 8080,\n  \"limits\": {\n    \"max_body_mb\": 16\n  }\n}\n"
        );
    }

    #[test]
    fn emit_json_orders_values_before_blocks() {
        // Arrange
        let fields = Fields::detached(vec![
            Field::detached_block(
                "sprocket",
                Fields::detached(vec![scalar("max_height", Scalar::Int(32))]),
            ),
            scalar("max_weight", Scalar::Int(16)),
        ]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"max_weight\": 16,\n  \"sprocket\": {\n    \"max_height\": 32\n  }\n}\n"
        );
    }

    #[test]
    fn emit_json_writes_an_empty_document_as_an_empty_object() {
        // Arrange
        let fields = Fields::detached(vec![]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{}\n");
        assert_eq!(reparse(&text).iter().count(), 0);
    }

    #[test]
    fn emit_json_writes_a_scalar_array_on_one_line() {
        // Arrange
        let fields = Fields::detached(vec![seq(
            "allow",
            vec![
                ValueKind::Scalar(Scalar::String("a".to_string())),
                ValueKind::Scalar(Scalar::String("b".to_string())),
            ],
        )]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{\n  \"allow\": [\"a\", \"b\"]\n}\n");
        let round = reparse(&text);
        let mut report = Report::new();
        let list = parse_string_list_field(round.get("allow").unwrap(), &mut report).unwrap();
        assert_eq!(list.value.len(), 2);
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_json_writes_an_empty_array_and_an_empty_object_inline() {
        // Arrange
        let fields = Fields::detached(vec![
            seq("allow", vec![]),
            Field::detached_value("tls", Value::detached(map(vec![]))),
        ]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{\n  \"allow\": [],\n  \"tls\": {}\n}\n");
        reparse(&text);
    }

    #[test]
    fn emit_json_writes_an_object_array_one_element_per_line() {
        // Arrange
        let element = |port: i64| map(vec![scalar("port", Scalar::Int(port))]);
        let fields = Fields::detached(vec![seq("service", vec![element(1), element(2)])]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"service\": [\n    {\n      \"port\": 1\n    },\n    {\n      \"port\": 2\n    }\n  ]\n}\n"
        );
        reparse(&text);
    }

    #[test]
    fn emit_json_writes_a_nested_array_one_element_per_line() {
        // Arrange
        let row = |first: i64| {
            ValueKind::Seq(vec![Value::detached(ValueKind::Scalar(Scalar::Int(first)))])
        };
        let fields = Fields::detached(vec![seq("matrix", vec![row(1), row(2)])]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{\n  \"matrix\": [\n    [1],\n    [2]\n  ]\n}\n");
        reparse(&text);
    }

    #[test]
    fn emit_json_groups_repeated_value_fields_into_one_array_member() {
        // Arrange
        // Only a parsed document with duplicate keys produces this shape.
        let fields = Fields::detached(vec![
            scalar("allow", Scalar::String("a".to_string())),
            scalar("name", Scalar::String("x".to_string())),
            scalar("allow", Scalar::String("b".to_string())),
        ]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"allow\": [\"a\", \"b\"],\n  \"name\": \"x\"\n}\n"
        );
    }

    #[test]
    fn emit_json_flattens_an_array_occurrence_into_the_grouped_array() {
        // Arrange
        // An array occurrence contributes its elements, and an object
        // occurrence contributes itself as one element.
        let fields = Fields::detached(vec![
            seq(
                "allow",
                vec![
                    ValueKind::Scalar(Scalar::Int(1)),
                    ValueKind::Scalar(Scalar::Int(2)),
                ],
            ),
            scalar("allow", Scalar::Int(3)),
            Field::detached_value("allow", Value::detached(map(vec![]))),
        ]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"allow\": [\n    1,\n    2,\n    3,\n    {}\n  ]\n}\n"
        );
    }

    #[test]
    fn emit_json_groups_repeated_blocks_into_one_array_of_objects() {
        // Arrange
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![block(1), block(2)]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"service\": [\n    {\n      \"port\": 1\n    },\n    {\n      \"port\": 2\n    }\n  ]\n}\n"
        );
        reparse(&text);
    }

    #[test]
    fn emit_json_groups_duplicates_inside_a_nested_object() {
        // Arrange
        let inner = Fields::detached(vec![
            scalar("allow", Scalar::String("a".to_string())),
            scalar("allow", Scalar::String("b".to_string())),
        ]);
        let fields = Fields::detached(vec![Field::detached_block("tls", inner)]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"tls\": {\n    \"allow\": [\"a\", \"b\"]\n  }\n}\n"
        );
        reparse(&text);
    }

    #[test]
    fn emit_json_rejects_a_native_label_it_cannot_write() {
        // Arrange
        // A parsed HCL or KDL block carries its label on the inner level, and
        // JSON has no label syntax and no field name to write it with.
        let inner = Fields::detached(vec![scalar("host", Scalar::String("h".to_string()))])
            .with_label(crate::source::Located::detached("api".to_string()));
        let fields = Fields::detached(vec![Field::detached_block("upstream", inner)]);

        // Act
        let result = emit_json(&fields);

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
    fn emit_json_rejects_a_value_beside_a_same_named_block() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![scalar("y", Scalar::Int(2))])),
        ]);

        // Act
        let result = emit_json(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: String::new(),
            })
        );
    }

    #[test]
    fn emit_json_names_the_enclosing_level_of_a_nested_conflict() {
        // Arrange
        let inner = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![])),
        ]);
        let fields = Fields::detached(vec![Field::detached_block("tls", inner)]);

        // Act
        let result = emit_json(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "tls".to_string(),
            })
        );
    }

    #[test]
    fn emit_json_rejects_an_unrepresentable_value_with_its_dotted_path() {
        // Arrange
        let inner = Fields::detached(vec![Field::detached_value(
            "when",
            Value::detached(ValueKind::Other("null")),
        )]);
        let fields = Fields::detached(vec![Field::detached_block("tls", inner)]);

        // Act
        let result = emit_json(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "null",
                path: "tls.when".to_string(),
            })
        );
    }

    #[test]
    fn emit_json_rejects_a_non_finite_float() {
        // JSON has no literal for infinity or NaN, so emit refuses rather than
        // changing the value.
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            // Arrange
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(value))]);

            // Act
            let result = emit_json(&fields);

            // Assert
            assert_eq!(
                result,
                Err(EmitError::UnrepresentableValue {
                    label: "non-finite float",
                    path: "rate".to_string(),
                }),
                "value {value} should be rejected"
            );
        }
    }

    #[test]
    fn emit_json_keeps_the_float_form() {
        // Arrange
        // Rust's shortest form of 1e20 uses an exponent rather than a
        // point, and the exponent is enough for the reparse to read a float.
        let fields = Fields::detached(vec![
            scalar("whole", Scalar::Float(1.0)),
            scalar("large", Scalar::Float(1e20)),
            scalar("count", Scalar::Int(4)),
        ]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "{\n  \"whole\": 1.0,\n  \"large\": 1e20,\n  \"count\": 4\n}\n"
        );
        let round = reparse(&text);
        let mut report = Report::new();
        for name in ["whole", "large"] {
            let ValueKind::Scalar(Scalar::Float(_)) = value_kind(&round, name) else {
                panic!("{name} should reparse as a float");
            };
        }
        assert_eq!(
            parse_float_field(round.get("large").unwrap(), &mut report)
                .unwrap()
                .value,
            1e20
        );
        assert!(!report.has_issues());
    }

    fn value_kind<'f>(fields: &'f Fields, name: &str) -> &'f ValueKind {
        let FieldKind::Value(value) = &fields.get(name).unwrap().kind else {
            panic!("{name} should be an attribute value");
        };
        &value.kind
    }

    #[test]
    fn emit_json_escapes_per_rfc_8259() {
        // Arrange
        // A quote, a backslash, each short escape, a control character with no
        // short escape, and raw UTF-8 that must pass through unescaped.
        let hostile = "quote\" backslash\\ nl\n tab\t cr\r bs\u{8} ff\u{c} \
                       nul\u{0} unit\u{1f} snowman\u{2603}";
        let fields = Fields::detached(vec![scalar(
            "greeting",
            Scalar::String(hostile.to_string()),
        )]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert!(
            text.contains(
                r#""quote\" backslash\\ nl\n tab\t cr\r bs\b ff\f nul\u0000 unit\u001f snowman☃""#
            ),
            "got: {text}"
        );
        let round = reparse(&text);
        let mut report = Report::new();
        let parsed = parse_string_field(round.get("greeting").unwrap(), &mut report).unwrap();
        assert_eq!(parsed.value, hostile);
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_json_escapes_a_key_the_same_way() {
        // Arrange
        let fields = Fields::detached(vec![scalar("weird\"key", Scalar::Int(1))]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{\n  \"weird\\\"key\": 1\n}\n");
        assert!(reparse(&text).get("weird\"key").is_some());
    }

    #[test]
    fn emit_json_writes_an_unparsed_scalar_as_a_string() {
        // Arrange
        // A layered tree carries unparsed text from an environment variable or
        // a flag, whose type was never decided.
        let fields = Fields::detached(vec![scalar("port", Scalar::Unparsed("8080".to_string()))]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{\n  \"port\": \"8080\"\n}\n");
    }

    #[test]
    fn emit_json_skips_commented_entries_and_renders_no_comments() {
        // Arrange
        // JSON has no comment syntax, so a template emits the same text as the
        // populated model.
        let annotated = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080))
                .with_doc(Some("The listen port.".to_string()))
                .into(),
            scalar("pid_file", Scalar::String(String::new()))
                .with_doc(Some("The PID file path.".to_string()))
                .as_commented(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
        ]);
        let plain = Fields::detached(vec![scalar("port", Scalar::Int(8080))]);

        // Act
        let text = emit_json(&annotated).unwrap();

        // Assert
        assert_eq!(text, emit_json(&plain).unwrap());
        assert_eq!(text, "{\n  \"port\": 8080\n}\n");
        assert!(!text.contains("//"), "got: {text}");
        assert!(!text.contains('#'), "got: {text}");
    }

    #[test]
    fn emit_json_excludes_commented_fields_from_grouping() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("x", Scalar::Int(1)).into(),
            scalar("x", Scalar::Int(2)).as_commented(),
        ]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{\n  \"x\": 1\n}\n");
    }

    #[test]
    fn emit_json_ignores_a_commented_entry_when_judging_a_conflict() {
        // Arrange
        // A commented entry is dropped rather than rendered, so it must not
        // count toward a conflict its active twin does not have.
        let fields = Fields::detached_entries(vec![
            scalar("x", Scalar::Int(1)).into(),
            Field::detached_block("x", Fields::detached(vec![])).as_commented(),
        ]);

        // Act
        let text = emit_json(&fields).unwrap();

        // Assert
        assert_eq!(text, "{\n  \"x\": 1\n}\n");
    }
}
