//! HCL write path: serializes a neutral [`Fields`] tree to canonical HCL.
//!
//! This is the inverse of [`parse_hcl_fields`](super::parse_hcl_fields). It
//! builds an `hcl-edit` `Body` by structure, indents each nesting level, and
//! renders the doc comments an annotated template carries.

use crate::format::EmitError;
use crate::format::emit::{child_path, comment_lines};
use crate::format::field::{FieldKind, Fields, Scalar, Value, ValueKind};
use hcl_edit::Decorate;
use hcl_edit::Ident;
use hcl_edit::expr::{Array, Expression, Object, ObjectKey, ObjectValue};
use hcl_edit::structure::{Attribute, Block, Body, Structure};

/// Serializes a [`Fields`] tree to canonical HCL text.
///
/// This is the inverse of [`parse_hcl_fields`](super::parse_hcl_fields). It
/// builds an `hcl-edit` `Body` by structure and returns its text, dropping the
/// comments and layout the neutral model never held. A nested struct emits as a
/// block, a repeated block emits once per element, and a non-identifier object
/// key is quoted. It fails on a non-identifier attribute or block name, which
/// HCL cannot spell, on a [`ValueKind::Other`], on two same-named values at one
/// level, which HCL rejects as duplicate attributes, and on any repeated name
/// inside an object. Those arise only when you emit a parsed or hand-built
/// `Fields`, not on the populate path. It also fails on the two numeric values
/// HCL has no literal for, an `i64::MIN` and a non-finite float, which a
/// populated spec can hold.
pub fn emit_hcl(fields: &Fields) -> Result<String, EmitError> {
    Ok(emit_body(fields, 0, "")?.to_string())
}

/// Builds a `Body` indented for the given nesting level.
///
/// Each structure is prefixed with `level` steps of two spaces. A block's inner
/// body carries a suffix of the block's own indent, which `hcl-edit` writes just
/// before the closing brace, so the brace lines up with the opener. Without the
/// suffix the brace would sit at column zero.
fn emit_body(fields: &Fields, level: usize, path: &str) -> Result<Body, EmitError> {
    // HCL repeats blocks freely and spells a value next to a block, but it
    // rejects a duplicate attribute, and hcl-edit would keep only the first.
    if let Some(name) = duplicate_attribute_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let indent = "  ".repeat(level);
    let mut body = Body::new();
    for field in fields.iter() {
        // The prefix carries the field's doc comment, if any, above its
        // indentation, so the comment aligns with the field it documents.
        let prefix = hcl_comment_prefix(&field.doc, &indent);
        match &field.kind {
            FieldKind::Value(value) => {
                let child = child_path(path, &field.name);
                let mut attribute =
                    Attribute::new(ident_of(&field.name, path)?, hcl_expr_of(value, &child)?);
                attribute.decor_mut().set_prefix(prefix);
                body.push(Structure::Attribute(attribute));
            }
            FieldKind::Block(inner) => {
                let child = child_path(path, &field.name);
                let mut block = Block::new(ident_of(&field.name, path)?);
                block.decor_mut().set_prefix(prefix);
                let mut inner_body = emit_body(inner, level + 1, &child)?;
                inner_body.decor_mut().set_suffix(indent.clone());
                block.body = inner_body;
                body.push(Structure::Block(block));
            }
        }
    }
    Ok(body)
}

/// The decor prefix for a field: its doc comment as `# line` comments, each at
/// the field's indentation, followed by the field's own indent. With no doc it
/// is just the indent.
fn hcl_comment_prefix(doc: &Option<String>, indent: &str) -> String {
    match doc {
        Some(text) => {
            let mut out = String::new();
            for line in comment_lines(text) {
                out.push_str(indent);
                if line.is_empty() {
                    out.push_str("#\n");
                } else {
                    out.push_str("# ");
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            out.push_str(indent);
            out
        }
        None => indent.to_string(),
    }
}

fn hcl_expr_of(value: &Value, path: &str) -> Result<Expression, EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => hcl_expr_of_scalar(scalar, path),
        ValueKind::Seq(elements) => {
            let mut array = Array::new();
            for element in elements {
                array.push(hcl_expr_of(element, path)?);
            }
            Ok(Expression::Array(array))
        }
        ValueKind::Map(inner) => Ok(Expression::Object(hcl_object_of(inner, path)?)),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// A name used by more than one value field at one level, which HCL cannot
/// spell as duplicate attributes.
fn duplicate_attribute_name(fields: &Fields) -> Option<&str> {
    fields.iter().find_map(|field| {
        let values = fields
            .iter()
            .filter(|other| other.name == field.name && matches!(other.kind, FieldKind::Value(_)))
            .count();
        (values > 1).then_some(field.name.as_str())
    })
}

/// A name repeated at all inside an object, which is a map with unique keys.
fn repeated_object_name(fields: &Fields) -> Option<&str> {
    fields.iter().find_map(|field| {
        let count = fields
            .iter()
            .filter(|other| other.name == field.name)
            .count();
        (count > 1).then_some(field.name.as_str())
    })
}

fn hcl_object_of(fields: &Fields, path: &str) -> Result<Object, EmitError> {
    if let Some(name) = repeated_object_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let mut object = Object::new();
    for field in fields.iter() {
        let child = child_path(path, &field.name);
        let value = match &field.kind {
            FieldKind::Value(value) => hcl_expr_of(value, &child)?,
            FieldKind::Block(inner) => Expression::Object(hcl_object_of(inner, &child)?),
        };
        object.insert(object_key_of(&field.name), ObjectValue::new(value));
    }
    Ok(object)
}

/// An identifier object key stays bare, and a non-identifier key is quoted as a
/// string expression, which HCL represents natively.
fn object_key_of(name: &str) -> ObjectKey {
    match Ident::try_new(name) {
        Ok(ident) => ObjectKey::Ident(ident.into()),
        Err(_) => ObjectKey::Expression(Expression::from(name.to_string())),
    }
}

fn hcl_expr_of_scalar(scalar: &Scalar, path: &str) -> Result<Expression, EmitError> {
    let expr = match scalar {
        Scalar::String(string) => Expression::from(string.clone()),
        Scalar::Int(int) => {
            // HCL reads a negative integer as a negation of its magnitude, and
            // i64::MIN's magnitude is 2^63, which overflows i64 on the way back
            // in. HCL has no literal that round-trips it, so refuse rather than
            // emit text the HCL parser cannot read. The upstream fix is
            // https://github.com/martinohmann/hcl-rs/pull/549. Once a released
            // hcl-edit round-trips i64::MIN, this rejection can be removed.
            if *int == i64::MIN {
                return Err(EmitError::UnrepresentableValue {
                    label: "i64::MIN",
                    path: path.to_string(),
                });
            }
            Expression::from(*int)
        }
        Scalar::Float(float) => {
            // HCL has no literal for infinity or NaN. hcl-edit maps a non-finite
            // float to `null`, so refuse rather than silently change the value.
            if !float.is_finite() {
                return Err(EmitError::UnrepresentableValue {
                    label: "non-finite float",
                    path: path.to_string(),
                });
            }
            // hcl-edit's own float conversion turns a whole-valued float into
            // an integer with a saturating cast, which corrupts a magnitude of
            // 2^63 or more and drops the float spelling everywhere else.
            // Parsing the float's shortest round-trip text instead keeps the
            // emitted literal exact, because a parsed expression renders its
            // own text verbatim.
            match format!("{float:?}").parse::<Expression>() {
                Ok(expression) => expression,
                // Unreachable: the debug form of a finite float is always a
                // valid HCL number or a negation of one.
                Err(_) => {
                    return Err(EmitError::UnrepresentableValue {
                        label: "float",
                        path: path.to_string(),
                    });
                }
            }
        }
        Scalar::Bool(boolean) => Expression::from(*boolean),
        Scalar::Unparsed(raw) => Expression::from(raw.clone()),
    };
    Ok(expr)
}

/// An attribute or block name must be a valid HCL identifier, because HCL has no
/// quoted spelling for one. A non-identifier name is unrepresentable.
fn ident_of(name: &str, path: &str) -> Result<Ident, EmitError> {
    Ident::try_new(name).map_err(|_| EmitError::UnrepresentableName {
        name: name.to_string(),
        path: path.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::parse_hcl_fields;
    use super::*;
    use crate::diagnostic::Report;
    use crate::format::field::{Field, FromFields};
    use crate::format::parse::{
        parse_float_field, parse_int_field, parse_string_field, parse_struct_list_field,
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
        let id = sources.add("emitted.hcl", text.to_string());
        let mut report = Report::new();
        let fields = parse_hcl_fields(&sources, id, &mut report).unwrap();
        assert!(
            !report.has_issues(),
            "reparse issues: {:?}",
            report.issues()
        );
        fields
    }

    #[test]
    fn emit_hcl_writes_canonical_text() {
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
        let text = emit_hcl(&fields).unwrap();
        // Assert
        // The block body is indented one level, and the closing brace lines up
        // with the opener.
        assert_eq!(
            text,
            "hostname = \"api\"\nport = 8080\nlimits {\n  max_body_mb = 16\n}\n"
        );
    }

    #[test]
    fn emit_hcl_round_trips_scalars_and_a_block() {
        let fields = Fields::detached(vec![
            scalar("name", Scalar::String("api".to_string())),
            scalar("count", Scalar::Int(42)),
            scalar("flag", Scalar::Bool(true)),
            Field::detached_block(
                "tls",
                Fields::detached(vec![scalar("cert", Scalar::String("a.pem".to_string()))]),
            ),
        ]);
        let round = reparse(&emit_hcl(&fields).unwrap());
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
    fn emit_hcl_writes_a_nested_list_as_repeated_blocks() {
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![block(1), block(2)]);
        let text = emit_hcl(&fields).unwrap();
        assert_eq!(text.matches("service {").count(), 2, "got: {text}");
        let round = reparse(&text);
        let mut report = Report::new();
        let mut services: Vec<Located<Probe>> = Vec::new();
        for field in round.iter() {
            parse_struct_list_field(&mut services, field, &mut report);
        }
        assert_eq!(services.len(), 2);
    }

    #[test]
    fn emit_hcl_rejects_a_non_identifier_attribute_name() {
        let fields = Fields::detached(vec![scalar("weird key", Scalar::Int(1))]);
        assert_eq!(
            emit_hcl(&fields),
            Err(EmitError::UnrepresentableName {
                name: "weird key".to_string(),
                path: String::new(),
            })
        );
    }

    #[test]
    fn emit_hcl_quotes_a_non_identifier_object_key() {
        let map = Fields::detached(vec![scalar("a b", Scalar::Int(1))]);
        let fields = Fields::detached(vec![Field::detached_value(
            "obj",
            Value::detached(ValueKind::Map(map)),
        )]);
        let text = emit_hcl(&fields).unwrap();
        assert!(text.contains("\"a b\""), "got: {text}");
        let round = reparse(&text);
        let FieldKind::Value(value) = &round.get("obj").unwrap().kind else {
            panic!("obj should be an attribute");
        };
        let ValueKind::Map(inner) = &value.kind else {
            panic!("obj should be a map");
        };
        assert!(inner.get("a b").is_some());
    }

    #[test]
    fn emit_hcl_rejects_an_unrepresentable_value() {
        let fields = Fields::detached(vec![Field::detached_value(
            "name",
            Value::detached(ValueKind::Other("string template")),
        )]);
        assert_eq!(
            emit_hcl(&fields),
            Err(EmitError::UnrepresentableValue {
                label: "string template",
                path: "name".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_rejects_i64_min() {
        // i64::MIN emits as `-9223372036854775808`, which HCL reads as a negation
        // of 2^63 and overflows on the way back in, so emit must refuse it rather
        // than produce text the HCL parser cannot read.
        let fields = Fields::detached(vec![scalar("offset", Scalar::Int(i64::MIN))]);
        assert_eq!(
            emit_hcl(&fields),
            Err(EmitError::UnrepresentableValue {
                label: "i64::MIN",
                path: "offset".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_rejects_a_non_finite_float() {
        // HCL has no literal for infinity or NaN, and hcl-edit would emit `null`,
        // so emit must refuse rather than silently change the value.
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(value))]);
            assert_eq!(
                emit_hcl(&fields),
                Err(EmitError::UnrepresentableValue {
                    label: "non-finite float",
                    path: "rate".to_string(),
                }),
                "value {value} should be rejected"
            );
        }
    }

    #[test]
    fn emit_hcl_rejects_two_attributes_sharing_a_name() {
        // Arrange
        // HCL rejects a duplicate attribute at parse time, and hcl-edit keeps
        // only the first on emit, so refusing beats losing one silently.
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            scalar("x", Scalar::Int(2)),
        ]);
        // Act
        let result = emit_hcl(&fields);
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
    fn emit_hcl_keeps_a_value_and_a_block_sharing_a_name() {
        // Arrange
        // HCL spells `x = 1` next to `x { }`, so the pair emits and reparses,
        // unlike in TOML where the same pair is refused.
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![scalar("y", Scalar::Int(2))])),
        ]);
        // Act
        let text = emit_hcl(&fields).unwrap();
        // Assert
        assert!(text.contains("x = 1"), "got: {text:?}");
        assert!(text.contains("x {"), "got: {text:?}");
        let round = reparse(&text);
        assert_eq!(round.iter().count(), 2);
    }

    #[test]
    fn emit_hcl_rejects_a_repeated_name_inside_an_object() {
        // Arrange
        // An object is a map, so a repeated key has no faithful spelling.
        let pair = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            scalar("x", Scalar::Int(2)),
        ]);
        let fields = Fields::detached(vec![Field::detached_value(
            "obj",
            Value::detached(ValueKind::Map(pair)),
        )]);
        // Act
        let result = emit_hcl(&fields);
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
    fn emit_hcl_names_the_nested_path_in_an_error() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_block(
            "limits",
            Fields::detached(vec![scalar("rate", Scalar::Float(f64::NAN))]),
        )]);
        // Act
        let result = emit_hcl(&fields);
        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "non-finite float",
                path: "limits.rate".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_writes_a_float_as_a_float_literal() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("whole", Scalar::Float(4.0)),
            scalar("fractional", Scalar::Float(1.5)),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // A whole-valued float keeps a float spelling, so the neutral model's
        // float kind survives the reparse instead of collapsing to an integer.
        assert_eq!(text, "whole = 4.0\nfractional = 1.5\n");
        let round = reparse(&text);
        for name in ["whole", "fractional"] {
            let FieldKind::Value(value) = &round.get(name).unwrap().kind else {
                panic!("{name} should be an attribute");
            };
            assert!(
                matches!(value.kind, ValueKind::Scalar(Scalar::Float(_))),
                "{name} should reparse as a float, got: {:?}",
                value.kind
            );
        }
    }

    #[test]
    fn emit_hcl_round_trips_a_float_beyond_i64_range() {
        // A whole float of magnitude 2^63 or more has no exact i64, so an
        // integer collapse would saturate and corrupt the value.
        let extremes = [1e19, -1e300, 9_223_372_036_854_775_808.0, 1.5e300];
        for expected in extremes {
            // Arrange
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(expected))]);

            // Act
            let text = emit_hcl(&fields).unwrap();

            // Assert
            let round = reparse(&text);
            let mut report = Report::new();
            let parsed = parse_float_field(round.get("rate").unwrap(), &mut report).unwrap();
            assert_eq!(parsed.value, expected, "emitted text: {text:?}");
            assert!(!report.has_issues());
        }
    }

    #[test]
    fn emit_hcl_normalizes_a_control_char_in_a_doc_comment() {
        // Arrange
        // A lone carriage return in a doc override would end an HCL comment early.
        let fields = Fields::detached(vec![
            scalar("port", Scalar::Int(1)).with_doc(Some("line one\rline two".to_string())),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // The carriage return became a second comment line, so the template reparses.
        assert!(text.contains("# line one\n"), "got: {text:?}");
        assert!(text.contains("# line two\n"), "got: {text:?}");
        reparse(&text);
    }
}
