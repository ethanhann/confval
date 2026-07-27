//! HCL write path: serializes a neutral [`Fields`] tree to canonical HCL.
//!
//! This is the inverse of [`parse_hcl_fields`](super::parse_hcl_fields). It
//! builds an `hcl-edit` `Body` by structure, indents each nesting level, and
//! renders the doc comments an annotated template carries.

use crate::format::EmitError;
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
/// HCL cannot spell, and on a [`ValueKind::Other`]. Neither arises on the
/// populate path.
pub fn emit_hcl(fields: &Fields) -> Result<String, EmitError> {
    Ok(emit_body(fields, 0)?.to_string())
}

/// Builds a `Body` indented for the given nesting level.
///
/// Each structure is prefixed with `level` steps of two spaces. A block's inner
/// body carries a suffix of the block's own indent, which `hcl-edit` writes just
/// before the closing brace, so the brace lines up with the opener. Without the
/// suffix the brace would sit at column zero.
fn emit_body(fields: &Fields, level: usize) -> Result<Body, EmitError> {
    let indent = "  ".repeat(level);
    let mut body = Body::new();
    for field in fields.iter() {
        // The prefix carries the field's doc comment, if any, above its
        // indentation, so the comment aligns with the field it documents.
        let prefix = hcl_comment_prefix(&field.doc, &indent);
        match &field.kind {
            FieldKind::Value(value) => {
                let mut attribute = Attribute::new(ident_of(&field.name)?, hcl_expr_of(value)?);
                attribute.decor_mut().set_prefix(prefix);
                body.push(Structure::Attribute(attribute));
            }
            FieldKind::Block(inner) => {
                let mut block = Block::new(ident_of(&field.name)?);
                block.decor_mut().set_prefix(prefix);
                let mut inner_body = emit_body(inner, level + 1)?;
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
            for line in text.split('\n') {
                out.push_str(indent);
                if line.is_empty() {
                    out.push_str("#\n");
                } else {
                    out.push_str("# ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out.push_str(indent);
            out
        }
        None => indent.to_string(),
    }
}

fn hcl_expr_of(value: &Value) -> Result<Expression, EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => Ok(hcl_expr_of_scalar(scalar)),
        ValueKind::Seq(elements) => {
            let mut array = Array::new();
            for element in elements {
                array.push(hcl_expr_of(element)?);
            }
            Ok(Expression::Array(array))
        }
        ValueKind::Map(inner) => Ok(Expression::Object(hcl_object_of(inner)?)),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue(label)),
    }
}

fn hcl_object_of(fields: &Fields) -> Result<Object, EmitError> {
    let mut object = Object::new();
    for field in fields.iter() {
        let value = match &field.kind {
            FieldKind::Value(value) => hcl_expr_of(value)?,
            FieldKind::Block(inner) => Expression::Object(hcl_object_of(inner)?),
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

fn hcl_expr_of_scalar(scalar: &Scalar) -> Expression {
    match scalar {
        Scalar::String(string) => Expression::from(string.clone()),
        Scalar::Int(int) => Expression::from(*int),
        Scalar::Float(float) => Expression::from(*float),
        Scalar::Bool(boolean) => Expression::from(*boolean),
        Scalar::Unparsed(raw) => Expression::from(raw.clone()),
    }
}

/// An attribute or block name must be a valid HCL identifier, because HCL has no
/// quoted spelling for one. A non-identifier name is unrepresentable.
fn ident_of(name: &str) -> Result<Ident, EmitError> {
    Ident::try_new(name).map_err(|_| EmitError::UnrepresentableName(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::super::parse_hcl_fields;
    use super::*;
    use crate::diagnostic::Report;
    use crate::format::field::{Field, FromFields};
    use crate::format::parse::{parse_int_field, parse_string_field, parse_struct_list_field};
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
            Err(EmitError::UnrepresentableName("weird key".to_string()))
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
            Err(EmitError::UnrepresentableValue("string template"))
        );
    }
}
