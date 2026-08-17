//! JSON frontend: parses strict JSON text into the format-neutral [`Fields`]
//! tree.
//!
//! This module's whole job is the conversion from jsonc-parser's AST to the
//! owned, format-neutral model in [`field`](crate::format::field). Once
//! [`parse_json`] hands back a `Fields`, every span has been captured and no
//! jsonc-parser type escapes. The leaf parsers, the derive-generated walks, and
//! the handwritten [`FromFields`] impls all work against the neutral model.
//!
//! The write path, [`emit_json`], lives in the sibling `emit` module.
//!
//! A JSON document holds one value. A configuration is a set of named fields,
//! so the root must be an object. Its members become the root level. Below the
//! root every object is a [`ValueKind::Map`], the shape TOML's inline table
//! and HCL's object attribute already lower to, so [`FieldKind::Block`] never
//! arises from a JSON parse. An array is a [`ValueKind::Seq`] with per-element
//! values, which a list field reads.
//!
//! The frontend accepts no jsonc-parser extension, so the text it reads and the
//! text it writes carry nothing a strict JSON parser rejects.
//!
//! Behavior contract:
//!
//! - Parsing accepts strict JSON alone. jsonc-parser's seven extensions are all
//!   turned off, so a comment, a trailing comma, an unquoted property name, a
//!   missing comma, a single-quoted string, a hexadecimal number, and a unary
//!   plus are each a syntax error.
//! - A syntax error is reported as one issue at its byte range, and parsing
//!   returns `None`.
//! - A root that is not an object and an empty document each report
//!   `expected an object at the document root` and return `None`.
//! - The frontend classifies a number by how it is written. Raw text with a `.`,
//!   an `e`, or an `E` is a float, and any other number is an integer. Two
//!   edges follow from that rule: `-0` has no float marker, so it reads as
//!   integer zero and its sign is not kept, and a decimal too small for `f64`,
//!   such as `1e-999`, underflows to `0.0` the way float parsing does.
//! - Values outside the neutral model (`null`, an integer beyond `i64`, a
//!   number whose `f64` value is not finite) become [`ValueKind::Other`]
//!   carrying a diagnostic label, so they surface as ordinary type mismatches
//!   at the field that used them.
//! - Duplicate keys stay separate fields. The generated walk resolves them by
//!   the spec's declared shape. A list field accumulates the occurrences in
//!   document order, and a single-value field reports a duplicate.

use crate::diagnostic::Report;
use crate::format::field::{Field, FieldKind, Fields, FromFields, Scalar, Value, ValueKind};
use crate::format::syntax::syntax_error;
use crate::source::{Source, SourceId, SourceMap, Span};
use jsonc_parser::CollectOptions;
use jsonc_parser::ParseOptions;
use jsonc_parser::ast::{Object, ObjectProp, ObjectPropName, Value as JsonValue};
use jsonc_parser::common::{Range, Ranged};
use jsonc_parser::parse_to_ast;

mod emit;
pub use emit::emit_json;

/// The message a root that cannot hold named fields reports.
const ROOT_MUST_BE_AN_OBJECT: &str = "expected an object at the document root";

/// Parses one registered source into the neutral [`Fields`] tree.
///
/// When you assemble configuration from several sources, you hold the returned
/// `Fields`, merge it with the others, and run [`FromFields`] once on the
/// merged result. A syntax error and a root that is not an object are the two
/// failures that yield no tree. Each is reported and returns `None`. Field-level
/// problems are reported but do not stop the parse, so a tree that parsed still
/// reaches validation.
pub fn parse_json_fields(sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
    let Some(source) = sources.get(id) else {
        report
            .error("internal error: parse_json_fields called with an unregistered source id")
            .emit();
        return None;
    };
    let document = Span::new(id, 0, source.text.len() as u32);
    let parsed = match parse_to_ast(&source.text, &CollectOptions::default(), &strict()) {
        Ok(parsed) => parsed,
        Err(error) => {
            report
                .error(syntax_error(&error.kind().to_string()))
                .at(error_span(error.range(), source, id))
                .emit();
            return None;
        }
    };
    match parsed.value {
        Some(JsonValue::Object(object)) => Some(fields_of_object(&object, document, id)),
        // A root that is not an object has no field names for the model to
        // hold.
        Some(other) => {
            report
                .error(ROOT_MUST_BE_AN_OBJECT)
                .at(span_of(other.range(), id))
                .emit();
            None
        }
        // An empty document has no value to point at, so the error takes the
        // whole document span.
        None => {
            report.error(ROOT_MUST_BE_AN_OBJECT).at(document).emit();
            None
        }
    }
}

/// Parses one registered source into a `T`, pushing syntax errors and
/// structural problems into the report.
pub fn parse_json<T: FromFields>(
    sources: &SourceMap,
    id: SourceId,
    report: &mut Report,
) -> Option<T> {
    let fields = parse_json_fields(sources, id, report)?;
    T::from_fields(&fields, report)
}

/// The parse options that accept strict JSON alone. Every one of jsonc-parser's
/// extensions defaults to accepting the loose form, so each is named here rather
/// than left to `Default`.
fn strict() -> ParseOptions {
    ParseOptions {
        allow_comments: false,
        allow_loose_object_property_names: false,
        allow_trailing_commas: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    }
}

/// Converts a jsonc-parser range to a confval [`Span`]. A `Range` holds byte
/// indices into the UTF-8 source, which `json_span_fidelity` pins, so the
/// conversion is a widening and nothing else.
fn span_of(range: Range, source: SourceId) -> Span {
    Span::new(source, range.start as u32, range.end as u32)
}

/// The span of a parse error, clamped inside the source. jsonc-parser reports
/// an empty range for a token it expected and did not find, such as the comma
/// between two members, so an empty range widens to one byte and stays visible
/// when rendered. An error at the end of input widens backward over the last
/// character rather than past the source.
fn error_span(range: Range, source: &Source, id: SourceId) -> Span {
    let len = source.text.len();
    let mut start = range.start.min(len);
    let mut end = if range.end > range.start {
        range.end.min(len)
    } else {
        start + 1
    };
    if end > len {
        end = len;
        start = source.floor_char_boundary(len.saturating_sub(1));
    }
    Span::new(id, start as u32, end as u32)
}

/// Normalizes an object's properties into neutral fields. `enclosing` is the
/// span missing-field errors point at. It is the object's brace range for a
/// nested level and the whole document at the root.
fn fields_of_object(object: &Object, enclosing: Span, source: SourceId) -> Fields {
    let items = object
        .properties
        .iter()
        .map(|property| field_of_property(property, source))
        .collect();
    Fields::new(source, enclosing, items)
}

/// Maps one property to one field. The name span covers the property name with
/// its quotes, matching how TOML spans a quoted key, and the field span covers
/// the name through the value.
fn field_of_property(property: &ObjectProp, source: SourceId) -> Field {
    Field::parsed(
        name_text(&property.name),
        span_of(property.name.range(), source),
        span_of(property.range, source),
        source,
        FieldKind::Value(value_of(&property.value, source)),
    )
}

/// A property name's text. The unquoted form is a JSONC extension this
/// frontend turns off, so only the quoted arm is reachable through
/// [`parse_json_fields`].
fn name_text<'p>(name: &'p ObjectPropName<'_>) -> &'p str {
    match name {
        ObjectPropName::String(literal) => literal.value.as_ref(),
        ObjectPropName::Word(literal) => literal.value,
    }
}

/// Converts one JSON value into a neutral [`Value`]. Anything the model has no
/// scalar for, `null` and a number outside the range its kind holds, becomes
/// [`ValueKind::Other`] with a diagnostic label.
fn value_of(value: &JsonValue, source: SourceId) -> Value {
    let span = span_of(value.range(), source);
    let kind = match value {
        // jsonc-parser resolves escape sequences, so the model holds the text
        // the operator meant rather than the escape text.
        JsonValue::StringLit(literal) => {
            ValueKind::Scalar(Scalar::String(literal.value.to_string()))
        }
        JsonValue::NumberLit(literal) => number_of(literal.value),
        JsonValue::BooleanLit(literal) => ValueKind::Scalar(Scalar::Bool(literal.value)),
        JsonValue::NullKeyword(_) => ValueKind::Other("null"),
        JsonValue::Array(array) => ValueKind::Seq(
            array
                .elements
                .iter()
                .map(|element| value_of(element, source))
                .collect(),
        ),
        JsonValue::Object(object) => ValueKind::Map(fields_of_object(object, span, source)),
    };
    Value { span, kind }
}

/// Classifies a number by how it is written. A fraction or an exponent makes a
/// float, and any other number is an integer. TOML draws the same distinction
/// syntactically, and the emitter preserves it.
///
/// A magnitude the chosen kind cannot hold surfaces as a type mismatch rather
/// than a distorted number. An integer beyond `i64` is an oversized integer,
/// and a float whose `f64` value overflows to infinity, such as `1e999`, is an
/// oversized number.
fn number_of(text: &str) -> ValueKind {
    if text.contains(['.', 'e', 'E']) {
        match text.parse::<f64>() {
            Ok(float) if float.is_finite() => ValueKind::Scalar(Scalar::Float(float)),
            _ => ValueKind::Other("oversized number"),
        }
    } else {
        match text.parse::<i64>() {
            Ok(int) => ValueKind::Scalar(Scalar::Int(int)),
            Err(_) => ValueKind::Other("oversized integer"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::parse::{
        parse_bool_field, parse_float_field, parse_int_field, parse_string_field,
        parse_string_list_field, parse_struct_field,
    };
    use crate::source::Located;

    struct Probe;
    impl FromFields for Probe {
        fn from_fields(_: &Fields, _: &mut Report) -> Option<Self> {
            Some(Probe)
        }
    }

    fn parse(input: &str) -> Fields {
        let mut sources = SourceMap::new();
        let id = sources.add("test.json", input);
        let mut report = Report::new();
        let fields = parse_json_fields(&sources, id, &mut report).unwrap();
        assert!(
            !report.has_issues(),
            "frontend reported: {:?}",
            report.issues()
        );
        fields
    }

    #[test]
    fn an_error_at_the_end_of_input_stays_inside_the_source() {
        // Arrange
        // jsonc-parser reports an empty range past the last byte for a token
        // it expected at the end, and the widened span must not run past the
        // source or split a character.
        for input in ["{\"a\":", "{\"a\": \u{20ac}"] {
            // Act
            let report = reject(input);

            // Assert
            let span = report.issues()[0].span.unwrap();
            assert!(
                input.get(span.start as usize..span.end as usize).is_some(),
                "span {}..{} is not a char-aligned range of the {}-byte input {input:?}",
                span.start,
                span.end,
                input.len()
            );
            assert!(span.end > span.start, "the span stays visible: {input:?}");
        }
    }

    fn reject(input: &str) -> Report {
        let mut sources = SourceMap::new();
        let id = sources.add("test.json", input);
        let mut report = Report::new();

        assert!(parse_json_fields(&sources, id, &mut report).is_none());
        report
    }

    #[test]
    fn scalars_parse_with_value_spans() {
        // Arrange
        let input = r#"{"hostname": "example.com", "port": 8080, "daemon": false, "ratio": 0.5}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let hostname = parse_string_field(fields.get("hostname").unwrap(), &mut report).unwrap();
        assert_eq!(hostname.value, "example.com");
        assert_eq!(
            &input[hostname.span.start as usize..hostname.span.end as usize],
            "\"example.com\""
        );
        let port = parse_int_field(fields.get("port").unwrap(), &mut report).unwrap();
        assert_eq!(port.value, 8080);
        assert!(
            !parse_bool_field(fields.get("daemon").unwrap(), &mut report)
                .unwrap()
                .value
        );
        let ratio = parse_float_field(fields.get("ratio").unwrap(), &mut report).unwrap();
        assert_eq!(ratio.value, 0.5);
        assert!(!report.has_issues());
    }

    #[test]
    fn an_array_parses_as_a_sequence_with_element_spans() {
        // Arrange
        let input = r#"{"allow": ["10.0.0.0/8", "192.168.0.0/16"]}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let list = parse_string_list_field(fields.get("allow").unwrap(), &mut report).unwrap();
        assert_eq!(list.value.len(), 2);
        let first = &list.value[0];
        assert_eq!(first.value, "10.0.0.0/8");
        assert_eq!(
            &input[first.span.start as usize..first.span.end as usize],
            "\"10.0.0.0/8\""
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn an_empty_array_parses_as_an_empty_sequence() {
        // Arrange
        let input = r#"{"allow": []}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let list = parse_string_list_field(fields.get("allow").unwrap(), &mut report).unwrap();
        assert!(list.value.is_empty());
        assert!(!report.has_issues());
    }

    #[test]
    fn a_nested_object_parses_as_a_map_the_struct_parser_accepts() {
        // Arrange
        let input = r#"{"tls": {"cert": "a.pem"}}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        assert!(parsed.is_some());
        let FieldKind::Value(value) = &fields.get("tls").unwrap().kind else {
            panic!("tls should be an attribute value");
        };
        let ValueKind::Map(inner) = &value.kind else {
            panic!("a nested object should be a map, never a block");
        };
        let cert = parse_string_field(inner.get("cert").unwrap(), &mut report).unwrap();
        assert_eq!(cert.value, "a.pem");
        assert!(!report.has_issues());
    }

    #[test]
    fn a_nested_object_carries_its_brace_range_as_enclosing() {
        // Arrange
        let input = r#"{"tls": {"cert": "a.pem"}}"#;

        // Act
        let fields = parse(input);

        // Assert
        let FieldKind::Value(value) = &fields.get("tls").unwrap().kind else {
            panic!("tls should be an attribute value");
        };
        let ValueKind::Map(inner) = &value.kind else {
            panic!("tls should be a map");
        };
        // A missing-field error inside `tls` points at the inner braces, not at
        // the whole document.
        assert_eq!(
            &input[inner.enclosing().start as usize..inner.enclosing().end as usize],
            r#"{"cert": "a.pem"}"#
        );
    }

    #[test]
    fn a_name_span_covers_the_quoted_key() {
        // Arrange
        let input = r#"{"port": 8080}"#;

        // Act
        let fields = parse(input);

        // Assert
        let field = fields.get("port").unwrap();
        assert_eq!(
            &input[field.name_span.start as usize..field.name_span.end as usize],
            "\"port\""
        );
        assert_eq!(
            &input[field.span.start as usize..field.span.end as usize],
            "\"port\": 8080"
        );
    }

    #[test]
    fn numbers_classify_by_how_they_are_written() {
        // Arrange
        // A whole-valued float keeps its float kind, so the emitted text
        // round-trips.
        let input = r#"{"count": 4, "whole": 4.0, "scaled": 4e2}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        assert_eq!(
            parse_int_field(fields.get("count").unwrap(), &mut report)
                .unwrap()
                .value,
            4
        );
        assert!(!report.has_issues());
        assert!(parse_int_field(fields.get("whole").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected integer, found number");
        let mut report = Report::new();
        assert_eq!(
            parse_float_field(fields.get("scaled").unwrap(), &mut report)
                .unwrap()
                .value,
            400.0
        );
    }

    #[test]
    fn a_negative_number_keeps_its_sign() {
        // Arrange
        let input = r#"{"offset": -12, "drift": -0.5}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        assert_eq!(
            parse_int_field(fields.get("offset").unwrap(), &mut report)
                .unwrap()
                .value,
            -12
        );
        assert_eq!(
            parse_float_field(fields.get("drift").unwrap(), &mut report)
                .unwrap()
                .value,
            -0.5
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn null_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        let input = r#"{"pid_file": null}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_string_field(fields.get("pid_file").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected string, found null");
    }

    #[test]
    fn an_oversized_integer_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        // i128 holds this, i64 does not.
        let input = r#"{"offset": 9223372036854775808}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_int_field(fields.get("offset").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected integer, found oversized integer"
        );
    }

    #[test]
    fn an_oversized_number_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        // f64 has no finite value for this magnitude, so the model refuses it
        // rather than holding infinity.
        let input = r#"{"ratio": 1e999}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_float_field(fields.get("ratio").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected number, found oversized number"
        );
    }

    #[test]
    fn a_scalar_where_a_nested_spec_is_expected_reports_the_shared_wording() {
        // Arrange
        // The expected side keeps the shared parsers' noun, so the message
        // names a block even though JSON has no blocks.
        let input = r#"{"tls": "a.pem"}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        assert!(parsed.is_none());
        assert_eq!(report.issues()[0].message, "expected block, found string");
    }

    #[test]
    fn duplicate_keys_stay_separate_fields_in_document_order() {
        // Arrange
        let input = r#"{"allow": "a", "port": 1, "allow": "b"}"#;

        // Act
        let fields = parse(input);

        // Assert
        let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["allow", "port", "allow"]);
        let values: Vec<String> = fields
            .iter()
            .filter(|field| field.name == "allow")
            .map(|field| {
                let FieldKind::Value(Value {
                    kind: ValueKind::Scalar(Scalar::String(text)),
                    ..
                }) = &field.kind
                else {
                    panic!("allow should be a string");
                };
                text.clone()
            })
            .collect();
        assert_eq!(values, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn a_nested_array_parses_as_a_sequence_of_sequences() {
        // Arrange
        let input = r#"{"matrix": [[1, 2], [3]]}"#;

        // Act
        let fields = parse(input);

        // Assert
        let FieldKind::Value(value) = &fields.get("matrix").unwrap().kind else {
            panic!("matrix should be an attribute value");
        };
        let ValueKind::Seq(rows) = &value.kind else {
            panic!("matrix should be a sequence");
        };
        assert_eq!(rows.len(), 2);
        let lengths: Vec<usize> = rows
            .iter()
            .map(|row| match &row.kind {
                ValueKind::Seq(cells) => cells.len(),
                _ => panic!("every row should be a sequence"),
            })
            .collect();
        assert_eq!(lengths, vec![2, 1]);
    }

    #[test]
    fn escapes_decode_and_later_spans_stay_byte_accurate() {
        // Arrange
        // A short escape, a four-digit escape, and a surrogate pair. The escape
        // text is longer than the value it stands for, so a span computed from
        // the decoded string rather than the source would drift on every field
        // after it.
        let input = r#"{"greeting": "a\nb\u00e9\ud83d\ude00", "port": 8080}"#;

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let greeting = parse_string_field(fields.get("greeting").unwrap(), &mut report).unwrap();
        assert_eq!(greeting.value, "a\nb\u{e9}\u{1f600}");
        let port = fields.get("port").unwrap();
        assert_eq!(
            &input[port.span.start as usize..port.span.end as usize],
            "\"port\": 8080"
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn spans_past_multibyte_text_are_byte_offsets() {
        // Arrange
        // The three-byte euro sign sits ahead of the second member, so a char
        // count would place its span two bytes early.
        let input = r#"{"cost": "€", "port": 8080}"#;

        // Act
        let fields = parse(input);

        // Assert
        let port = fields.get("port").unwrap();
        assert_eq!(
            &input[port.name_span.start as usize..port.name_span.end as usize],
            "\"port\""
        );
    }

    #[test]
    fn a_root_array_reports_at_the_root_value() {
        // Arrange
        let input = "[1, 2]";

        // Act
        let report = reject(input);

        // Assert
        assert_eq!(report.issues()[0].message, ROOT_MUST_BE_AN_OBJECT);
        let span = report.issues()[0].span.unwrap();
        assert_eq!(&input[span.start as usize..span.end as usize], "[1, 2]");
    }

    #[test]
    fn a_root_string_reports_at_the_root_value() {
        // Arrange
        let input = r#""just text""#;

        // Act
        let report = reject(input);

        // Assert
        assert_eq!(report.issues()[0].message, ROOT_MUST_BE_AN_OBJECT);
        let span = report.issues()[0].span.unwrap();
        assert_eq!(
            &input[span.start as usize..span.end as usize],
            r#""just text""#
        );
    }

    #[test]
    fn an_empty_document_reports_at_the_whole_document() {
        // Arrange
        for input in ["", "  \n  "] {
            // Act
            let report = reject(input);

            // Assert
            assert_eq!(report.issues()[0].message, ROOT_MUST_BE_AN_OBJECT);
            let span = report.issues()[0].span.unwrap();
            assert_eq!(span.start, 0);
            assert_eq!(span.end as usize, input.len());
        }
    }

    #[test]
    fn each_loose_form_is_a_syntax_error() {
        // Arrange
        // One case per jsonc-parser extension flag, every one of which defaults
        // to accepting the loose form.
        let loose = [
            r#"{"port": 8080} // trailing comment"#,
            r#"{"port": 8080,}"#,
            r#"{port: 8080}"#,
            r#"{"port": 8080 "host": "a"}"#,
            r#"{'port': 8080}"#,
            r#"{"port": 0xFF}"#,
            r#"{"port": +8080}"#,
        ];

        for input in loose {
            // Act
            let report = reject(input);

            // Assert
            assert!(
                report.issues()[0].message.starts_with("syntax error: "),
                "{input} should be a syntax error, got: {:?}",
                report.issues()
            );
            assert!(report.issues()[0].span.is_some(), "input: {input}");
        }
    }

    #[test]
    fn a_syntax_error_reports_one_issue_at_its_byte_range() {
        // Arrange
        let input = "{\"cost\": \"€\", \"port\" 8080}";

        // Act
        let report = reject(input);

        // Assert
        assert_eq!(report.issues().len(), 1);
        let span = report.issues()[0].span.unwrap();
        assert!(
            input.is_char_boundary(span.start as usize)
                && input.is_char_boundary(span.end as usize),
            "span {}..{} is not on char boundaries",
            span.start,
            span.end
        );
        assert!(span.start as usize >= input.find("\"port\"").unwrap());
    }
}
