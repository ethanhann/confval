//! HCL frontend: parses HCL text into the format-neutral [`Fields`] tree.
//!
//! This module's whole job is the conversion from `hcl_edit`'s syntax tree to
//! the owned, format-neutral model in [`field`](crate::format::field). Once
//! [`parse_hcl`] hands back a `Fields`, every span has been captured and no
//! `hcl_edit` type escapes. The leaf parsers, the derive-generated walks, and
//! the handwritten [`FromFields`] impls all work against the neutral model.
//!
//! The write path, [`emit_hcl`], is in the sibling `emit` module.
//!
//! HCL offers two ways to write a nested structure: blocks (`server { ... }`)
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
use crate::format::field::{Field, FieldKind, Fields, FromFields, Scalar, Value, ValueKind};
use crate::format::syntax::syntax_error;
use crate::source::{Located, SourceId, SourceMap, Span};
use hcl_edit::expr::{Expression, Object, ObjectKey};
use hcl_edit::structure::{Body, Structure};

mod commented;
mod emit;
pub use emit::emit_hcl;

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
            Some(fields_of_body(&body, enclosing, &source.text, id, report))
        }
        Err(error) => {
            let offset = error.location().offset() as u32;
            report
                .error(syntax_error(error.message()))
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
fn span_of(node: &impl hcl_edit::Span, source: SourceId) -> Span {
    match node.span() {
        Some(range) => Span::new(source, range.start as u32, range.end as u32),
        None => Span::detached(),
    }
}

/// Normalizes a body's attributes and blocks into neutral fields. `enclosing`
/// is the span missing-field errors point at: the surrounding block, or the
/// whole file at the root. `text` is the source's full text, which numeric
/// literals are re-read from.
fn fields_of_body(
    body: &Body,
    enclosing: Span,
    text: &str,
    source: SourceId,
    report: &mut Report,
) -> Fields {
    let mut items = Vec::new();
    for structure in body.iter() {
        match structure {
            Structure::Attribute(attr) => items.push(Field::parsed(
                attr.key.value().as_str(),
                span_of(&attr.key, source),
                span_of(attr, source),
                source,
                FieldKind::Value(value_of_expr(&attr.value, text, source, report)),
            )),
            Structure::Block(block) => {
                let block_span = span_of(block, source);
                let mut body = fields_of_body(&block.body, block_span, text, source, report);
                let mut labels = block.labels.iter();
                if let Some(label) = labels.next() {
                    body = body.with_label(Located::new(
                        label.as_str().to_string(),
                        span_of(label, source),
                    ));
                }
                for extra in labels {
                    report
                        .error("a block label must be the only one")
                        .at(span_of(extra, source))
                        .emit();
                }
                items.push(Field::parsed(
                    block.ident.value().as_str(),
                    span_of(&block.ident, source),
                    block_span,
                    source,
                    FieldKind::Block(body),
                ));
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
    text: &str,
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
        let value = value_of_expr(value.expr(), text, source, report);
        items.push(Field::parsed(
            name,
            name_span,
            Span::merge(name_span, value.span),
            source,
            FieldKind::Value(value),
        ));
    }
    Fields::new(source, enclosing, items)
}

/// Converts one HCL expression into a neutral [`Value`], recursing through
/// arrays and objects. Anything the model has no scalar for (a template, null)
/// becomes [`ValueKind::Other`] with a diagnostic label.
fn value_of_expr(expr: &Expression, text: &str, source: SourceId, report: &mut Report) -> Value {
    let span = span_of(expr, source);
    let kind = if let Some(string) = expr.as_str() {
        ValueKind::Scalar(Scalar::String(string.to_string()))
    } else if let Some(boolean) = expr.as_bool() {
        ValueKind::Scalar(Scalar::Bool(boolean))
    } else if let Some(number) = expr.as_number() {
        scalar_of_number(number, span, text)
    } else if let Some(array) = expr.as_array() {
        ValueKind::Seq(
            array
                .iter()
                .map(|element| value_of_expr(element, text, source, report))
                .collect(),
        )
    } else if let Expression::Object(object) = expr {
        ValueKind::Map(fields_of_object(object, span, text, source, report))
    } else {
        ValueKind::Other(describe_other(expr))
    };
    Value { span, kind }
}

/// Converts a parsed number into a value kind. hcl-edit collapses a
/// whole-valued float literal into an integer with a saturating cast, and it
/// saturates an integer literal past the `i64` range instead of refusing it,
/// which silently changes the number the author wrote. The literal's own text
/// is the authority: a literal written as a float, with a dot or an exponent,
/// is re-read from the source, and an integer literal is re-read the same
/// way, so a value the text cannot hold reports as an oversized integer, the
/// diagnostic the other frontends produce for the same input. A negation may
/// carry whitespace between the sign and the digits, which the standard
/// parsers do not accept, so the literal is compacted first.
fn scalar_of_number(number: &hcl_edit::Number, span: Span, text: &str) -> ValueKind {
    let compact: String = text
        .get(span.start as usize..span.end as usize)
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    if compact.contains(['.', 'e', 'E']) {
        if let Ok(float) = compact.parse::<f64>() {
            return ValueKind::Scalar(Scalar::Float(float));
        }
    } else if !compact.is_empty() {
        return match compact.parse::<i64>() {
            Ok(int) => ValueKind::Scalar(Scalar::Int(int)),
            Err(_) => ValueKind::Other("oversized integer"),
        };
    }
    match (number.as_i64(), number.as_f64()) {
        (Some(int), _) => ValueKind::Scalar(Scalar::Int(int)),
        (None, Some(float)) => ValueKind::Scalar(Scalar::Float(float)),
        (None, None) => ValueKind::Other("number"),
    }
}

/// Diagnostic label for an expression the neutral model cannot represent.
fn describe_other(expr: &Expression) -> &'static str {
    match expr {
        Expression::Null(_) => "null",
        Expression::StringTemplate(_) | Expression::HeredocTemplate(_) => "string template",
        _ => "expression",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::parse::{
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
        let text = sources.get(id).unwrap().text.clone();
        let fields = fields_of_body(&body, Span::new(id, 0, 0), &text, id, &mut report);
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
    fn float_literal_beyond_i64_range_parses_exactly() {
        // Arrange
        // hcl-edit collapses a whole-valued float to an integer with a
        // saturating cast, so these literals corrupt unless the value is
        // recovered from the source text.
        let input = "rate = 1e19\nfloor = -1e300\n";

        // Act
        let (_, _, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        let rate = parse_float_field(fields.get("rate").unwrap(), &mut report).unwrap();
        let floor = parse_float_field(fields.get("floor").unwrap(), &mut report).unwrap();
        assert_eq!(rate.value, 1e19);
        assert_eq!(floor.value, -1e300);
        assert!(!report.has_issues());
    }

    #[test]
    fn whole_float_literal_keeps_the_float_kind() {
        // Arrange
        let input = "ratio = 4.0\n";

        // Act
        let (_, _, fields) = parse(input);

        // Assert
        // The literal is written as a float, so the neutral model keeps the float
        // kind instead of collapsing to an integer.
        let FieldKind::Value(value) = &fields.get("ratio").unwrap().kind else {
            panic!("ratio should be a value");
        };
        assert!(
            matches!(value.kind, ValueKind::Scalar(Scalar::Float(f)) if f == 4.0),
            "ratio should stay a float, got: {:?}",
            value.kind
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
    fn an_oversized_integer_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        // i128 holds these, i64 does not, and hcl-edit saturates rather than
        // refusing, so the literal's own text is the authority.
        let input = "offset = -9223372036854775809\nlimit = 9223372036854775808\n";

        // Act
        let (_, _, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_int_field(fields.get("offset").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected integer, found oversized integer"
        );
        let mut report = Report::new();
        assert!(parse_int_field(fields.get("limit").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected integer, found oversized integer"
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
    fn unknown_block_reported_with_block_label() {
        let (_, id, fields) = parse("tsl {\n}\n");
        let mut report = Report::new();
        report_unknown_field(fields.get("tsl").unwrap(), &mut report);
        assert_eq!(report.issues()[0].message, "unknown block: tsl");
        assert_eq!(report.issues()[0].span, Some(Span::new(id, 0, 3)));
    }

    #[test]
    fn null_value_becomes_other_with_its_own_label() {
        // Arrange
        // The model has no null, and the label is what the format limitations
        // page names as HCL's observable, so it needs its own pin. Without one,
        // dropping the arm lets `null` report as a generic expression.
        let (_, _, fields) = parse("pid_file = null\n");

        // Act
        let mut report = Report::new();
        let parsed = parse_string_field(fields.get("pid_file").unwrap(), &mut report);

        // Assert
        assert!(parsed.is_none());
        assert_eq!(report.issues()[0].message, "expected string, found null");
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
}
