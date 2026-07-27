//! HCL frontend: parses HCL text into the format-neutral [`Fields`] tree.
//!
//! This module's whole job is the conversion from `hcl_edit`'s syntax tree to
//! the owned, format-neutral model in [`field`](crate::format::field). Once
//! [`parse_hcl`] hands back a `Fields`, every span has been captured and no
//! `hcl_edit` type escapes. The leaf parsers, the derive-generated walks, and
//! the hand-written [`FromFields`] impls all work against the neutral model.
//!
//! HCL offers two spellings for nested structures: blocks (`server { ... }`)
//! and object-valued attributes (`server = { ... }`). A block becomes a
//! [`FieldKind::Block`]. An object attribute becomes a [`FieldKind::Value`]
//! whose value is a [`ValueKind::Map`]. Both reach the same `FromFields` impl,
//! and the leaf parsers accept either.
//!
//! Behavior contract:
//!
//! - Syntax errors are pushed to the report with the parser's location and
//!   parsing returns `None`.
//! - Values outside the neutral model (HCL templates, null) become
//!   [`ValueKind::Other`] carrying a diagnostic label, so they surface as
//!   ordinary type mismatches at the field that used them.
//! - Non-identifier, non-string object keys are reported and skipped.

use crate::diagnostic::Report;
use crate::format::EmitError;
use crate::format::field::{Field, FieldKind, Fields, FromFields, Scalar, Value, ValueKind};
use crate::source::{SourceId, SourceMap, Span};
use hcl_edit::Ident;
use hcl_edit::expr::{Array, Expression, Object, ObjectKey, ObjectValue};
use hcl_edit::structure::{Attribute, Block, Body, Structure};

/// Parses one registered source into the neutral [`Fields`] tree.
///
/// When you assemble configuration from several sources, you hold the returned
/// `Fields`, merge it with the others, and run [`FromFields`] once on the
/// merged result. A syntax error, the only failure that yields no tree, is
/// reported and returns `None`. Field-level problems are reported but do not
/// stop the parse, so a tree that parsed still reaches validation.
pub fn parse_hcl_fields(sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
    let Some(source) = sources.get(id) else {
        report
            .error("internal error: parse_hcl_fields called with an unregistered source id")
            .emit();
        return None;
    };
    match hcl_edit::parser::parse_body(&source.text) {
        Ok(body) => {
            let enclosing = Span::new(id, 0, source.text.len() as u32);
            Some(fields_of_body(&body, enclosing, id, report))
        }
        Err(error) => {
            let offset = error.location().offset() as u32;
            report
                .error(format!("syntax error: {}", error.message()))
                .at(Span::new(id, offset, offset.saturating_add(1)))
                .emit();
            None
        }
    }
}

/// Parses one registered source into a `T`, pushing syntax errors and
/// structural problems into the report.
pub fn parse_hcl<T: FromFields>(
    sources: &SourceMap,
    id: SourceId,
    report: &mut Report,
) -> Option<T> {
    let fields = parse_hcl_fields(sources, id, report)?;
    T::from_fields(&fields, report)
}

/// Converts an hcl-edit node's span to a confval [`Span`]. Nodes not emitted by
/// the parser have no span and map to a detached one.
pub fn span_of(node: &impl hcl_edit::Span, source: SourceId) -> Span {
    match node.span() {
        Some(range) => Span::new(source, range.start as u32, range.end as u32),
        None => Span::detached(),
    }
}

/// Normalizes a body's attributes and blocks into neutral fields. `enclosing`
/// is the span missing-field errors point at: the surrounding block, or the
/// whole file at the root.
fn fields_of_body(body: &Body, enclosing: Span, source: SourceId, report: &mut Report) -> Fields {
    let mut items = Vec::new();
    for structure in body.iter() {
        match structure {
            Structure::Attribute(attr) => items.push(Field {
                name: attr.key.value().as_str().to_string(),
                name_span: span_of(&attr.key, source),
                span: span_of(attr, source),
                source,
                kind: FieldKind::Value(value_of_expr(&attr.value, source, report)),
            }),
            Structure::Block(block) => {
                let block_span = span_of(block, source);
                items.push(Field {
                    name: block.ident.value().as_str().to_string(),
                    name_span: span_of(&block.ident, source),
                    span: block_span,
                    source,
                    kind: FieldKind::Block(fields_of_body(&block.body, block_span, source, report)),
                });
            }
        }
    }
    Fields::new(source, enclosing, items)
}

/// Normalizes an object's items into neutral fields. Non-identifier,
/// non-string keys are reported and skipped.
fn fields_of_object(
    object: &Object,
    enclosing: Span,
    source: SourceId,
    report: &mut Report,
) -> Fields {
    let mut items = Vec::new();
    for (key, value) in object.iter() {
        let name = match key {
            ObjectKey::Ident(ident) => ident.value().as_str(),
            ObjectKey::Expression(expr) => match expr.as_str() {
                Some(name) => name,
                None => {
                    report
                        .error("expected an identifier or string as object key")
                        .at(span_of(key, source))
                        .emit();
                    continue;
                }
            },
        };
        let name_span = span_of(key, source);
        let value = value_of_expr(value.expr(), source, report);
        items.push(Field {
            name: name.to_string(),
            name_span,
            span: Span::merge(name_span, value.span),
            source,
            kind: FieldKind::Value(value),
        });
    }
    Fields::new(source, enclosing, items)
}

/// Converts one HCL expression into a neutral [`Value`], recursing through
/// arrays and objects. Anything the model has no scalar for (a template, null)
/// becomes [`ValueKind::Other`] with a diagnostic label.
fn value_of_expr(expr: &Expression, source: SourceId, report: &mut Report) -> Value {
    let span = span_of(expr, source);
    let kind = if let Some(string) = expr.as_str() {
        ValueKind::Scalar(Scalar::String(string.to_string()))
    } else if let Some(boolean) = expr.as_bool() {
        ValueKind::Scalar(Scalar::Bool(boolean))
    } else if let Some(number) = expr.as_number() {
        if let Some(int) = number.as_i64() {
            ValueKind::Scalar(Scalar::Int(int))
        } else if let Some(float) = number.as_f64() {
            ValueKind::Scalar(Scalar::Float(float))
        } else {
            ValueKind::Other("number")
        }
    } else if let Some(array) = expr.as_array() {
        ValueKind::Seq(
            array
                .iter()
                .map(|element| value_of_expr(element, source, report))
                .collect(),
        )
    } else if let Expression::Object(object) = expr {
        ValueKind::Map(fields_of_object(object, span, source, report))
    } else {
        ValueKind::Other(describe_other(expr))
    };
    Value { span, kind }
}

/// Diagnostic label for an expression the neutral model cannot represent.
fn describe_other(expr: &Expression) -> &'static str {
    match expr {
        Expression::Null(_) => "null",
        Expression::StringTemplate(_) | Expression::HeredocTemplate(_) => "string template",
        _ => "expression",
    }
}

/// Serializes a [`Fields`] tree to canonical HCL text.
///
/// This is the inverse of [`parse_hcl_fields`]. It builds an `hcl-edit` `Body`
/// by structure and returns its text, dropping the comments and layout the
/// neutral model never held. A nested struct emits as a block, a repeated block
/// emits once per element, and a non-identifier object key is quoted. It fails
/// on a non-identifier attribute or block name, which HCL cannot spell, and on a
/// [`ValueKind::Other`]. Neither arises on the populate path.
pub fn emit_hcl(fields: &Fields) -> Result<String, EmitError> {
    Ok(emit_body(fields)?.to_string())
}

fn emit_body(fields: &Fields) -> Result<Body, EmitError> {
    let mut body = Body::new();
    for field in fields.iter() {
        match &field.kind {
            FieldKind::Value(value) => {
                body.push(Structure::Attribute(Attribute::new(
                    ident_of(&field.name)?,
                    hcl_expr_of(value)?,
                )));
            }
            FieldKind::Block(inner) => {
                let mut block = Block::new(ident_of(&field.name)?);
                block.body = emit_body(inner)?;
                body.push(Structure::Block(block));
            }
        }
    }
    Ok(body)
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
        let id = sources.add("test.hcl", input);
        let body = hcl_edit::parser::parse_body(&sources.get(id).unwrap().text).unwrap();
        let mut report = Report::new();
        let fields = fields_of_body(&body, Span::new(id, 0, 0), id, &mut report);
        assert!(
            !report.has_issues(),
            "frontend reported: {:?}",
            report.issues()
        );
        (sources, id, fields)
    }

    #[test]
    fn string_field_parses_with_value_span() {
        let (_, id, fields) = parse("name = \"api\"\n");
        let mut report = Report::new();
        let value = parse_string_field(fields.get("name").unwrap(), &mut report).unwrap();
        assert_eq!(value.value, "api");
        assert_eq!(value.span, Span::new(id, 7, 12));
        assert!(!report.has_issues());
    }

    #[test]
    fn string_field_type_mismatch_reports_at_value_span() {
        let (_, id, fields) = parse("name = 42\n");
        let mut report = Report::new();
        assert!(parse_string_field(fields.get("name").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected string, found number");
        assert_eq!(report.issues()[0].span, Some(Span::new(id, 7, 9)));
    }

    #[test]
    fn int_field_parses_and_rejects_floats() {
        let (_, _, fields) = parse("port = 8080\nratio = 1.5\n");
        let mut report = Report::new();
        let port = parse_int_field(fields.get("port").unwrap(), &mut report);
        assert_eq!(port.unwrap().value, 8080);
        assert!(parse_int_field(fields.get("ratio").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected integer, found number");
    }

    #[test]
    fn float_field_widens_integers() {
        let (_, _, fields) = parse("ratio = 0.5\nwhole = 1\n");
        let mut report = Report::new();
        let ratio = parse_float_field(fields.get("ratio").unwrap(), &mut report);
        let whole = parse_float_field(fields.get("whole").unwrap(), &mut report);
        assert_eq!(ratio.unwrap().value, 0.5);
        assert_eq!(whole.unwrap().value, 1.0);
        assert!(!report.has_issues());
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
    fn struct_field_accepts_block_form() {
        let input = "tls {\n  cert = \"a.pem\"\n}\n";
        let (_, _, fields) = parse(input);
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        let span = parsed.unwrap().span;
        assert_eq!(
            &input[span.start as usize..span.end as usize],
            "tls {\n  cert = \"a.pem\"\n}"
        );
    }

    #[test]
    fn struct_field_accepts_object_form() {
        let (_, _, fields) = parse("tls = {\n  cert = \"a.pem\"\n}\n");
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        assert!(parsed.is_some());
        assert!(!report.has_issues());
    }

    #[test]
    fn object_items_have_name_and_value_spans() {
        let input = "tls = {\n  cert = \"a.pem\"\n}\n";
        let (_, _, fields) = parse(input);
        let FieldKind::Value(value) = &fields.get("tls").unwrap().kind else {
            panic!("expected attribute value");
        };
        let ValueKind::Map(inner) = &value.kind else {
            panic!("expected map value");
        };
        let cert = inner.get("cert").unwrap();
        assert_eq!(
            &input[cert.name_span.start as usize..cert.name_span.end as usize],
            "cert"
        );
        let mut report = Report::new();
        let parsed = parse_string_field(cert, &mut report).unwrap();
        assert_eq!(
            &input[parsed.span.start as usize..parsed.span.end as usize],
            "\"a.pem\""
        );
    }

    #[test]
    fn struct_list_appends_repeated_blocks() {
        let input = "service {\n  a = 1\n}\nservice {\n  b = 2\n}\n";
        let (_, _, fields) = parse(input);
        let mut report = Report::new();
        let mut services: Vec<Located<Probe>> = Vec::new();
        for field in fields.iter() {
            parse_struct_list_field(&mut services, field, &mut report);
        }
        assert_eq!(services.len(), 2);
        assert!(!report.has_issues());
        let second = &input[services[1].span.start as usize..services[1].span.end as usize];
        assert!(second.contains("b = 2"), "got: {second:?}");
    }

    #[test]
    fn struct_list_accepts_array_of_objects() {
        let input = "services = [\n  { a = 1 },\n  { b = 2 },\n]\n";
        let (_, _, fields) = parse(input);
        let mut report = Report::new();
        let mut services: Vec<Located<Probe>> = Vec::new();
        parse_struct_list_field(&mut services, fields.get("services").unwrap(), &mut report);
        assert_eq!(services.len(), 2);
        assert!(!report.has_issues());
        let first = &input[services[0].span.start as usize..services[0].span.end as usize];
        assert_eq!(first, "{ a = 1 }");
    }

    #[test]
    fn syntax_error_is_reported_with_location() {
        let mut sources = SourceMap::new();
        let id = sources.add("broken.hcl", "server {\n  port =\n");
        let mut report = Report::new();
        let parsed: Option<Probe> = parse_hcl(&sources, id, &mut report);
        assert!(parsed.is_none());
        assert!(report.has_errors());
        assert!(report.issues()[0].message.starts_with("syntax error:"));
        assert!(report.issues()[0].span.is_some());
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
    fn unknown_block_reported_with_block_label() {
        let (_, id, fields) = parse("tsl {\n}\n");
        let mut report = Report::new();
        report_unknown_field(fields.get("tsl").unwrap(), &mut report);
        assert_eq!(report.issues()[0].message, "unknown block: tsl");
        assert_eq!(report.issues()[0].span, Some(Span::new(id, 0, 3)));
    }

    #[test]
    fn template_value_becomes_other() {
        // A string interpolation has no static value, so it must surface as a
        // type mismatch, not silently parse.
        let (_, _, fields) = parse("name = \"${var.x}\"\n");
        let mut report = Report::new();
        assert!(parse_string_field(fields.get("name").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected string, found string template"
        );
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
        // hcl-edit emits a programmatically built block body without
        // indentation. It is valid HCL and round-trips. Prettier layout is a
        // formatting concern the milestone leaves out.
        assert_eq!(
            text,
            "hostname = \"api\"\nport = 8080\nlimits {\nmax_body_mb = 16\n}\n"
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
