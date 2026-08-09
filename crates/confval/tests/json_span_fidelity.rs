//! Guards the span fidelity the `confval::format::json` adapter depends on. The
//! adapter needs jsonc-parser to expose byte-accurate ranges for property
//! names, whole properties, values, and object braces, plus a byte range on the
//! parse error. If a jsonc-parser upgrade breaks any of these, error
//! attribution breaks with it.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "json")]

use jsonc_parser::ParseOptions;
use jsonc_parser::ast::{Object, ObjectProp, ObjectPropName, Value};
use jsonc_parser::common::Ranged;
use jsonc_parser::parse_to_ast;

/// The euro sign ahead of every other member makes a char count disagree with
/// a byte index by two from `port` onward, so an assertion over this input
/// fails if the ranges are anything but byte indices.
const INPUT: &str = r#"{
  "cost": "€",
  "port": 8080,
  "tls": {
    "cert": "cert.pem"
  },
  "allow": ["10.0.0.0/8", "192.168.0.0/16"]
}
"#;

/// The options the frontend parses with: strict JSON, every extension off.
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

fn root() -> Object<'static> {
    let parsed = parse_to_ast(INPUT, &Default::default(), &strict()).unwrap();
    match parsed.value.expect("the document has a root value") {
        Value::Object(object) => object,
        other => panic!("the root should be an object, got {other:?}"),
    }
}

fn name_of<'p>(property: &'p ObjectProp<'_>) -> &'p str {
    match &property.name {
        ObjectPropName::String(literal) => literal.value.as_ref(),
        ObjectPropName::Word(literal) => literal.value,
    }
}

fn property<'o>(object: &'o Object<'_>, name: &str) -> &'o ObjectProp<'o> {
    object
        .properties
        .iter()
        .find(|property| name_of(property) == name)
        .unwrap_or_else(|| panic!("{name} should be present"))
}

fn slice(ranged: &impl Ranged) -> &'static str {
    &INPUT[ranged.start()..ranged.end()]
}

#[test]
fn property_name_ranges_include_the_quotes() {
    // Arrange
    let object = root();

    // Act
    let port = property(&object, "port");

    // Assert
    assert_eq!(slice(&port.name), "\"port\"");
}

#[test]
fn property_ranges_cover_the_name_through_the_value() {
    // Arrange
    let object = root();

    // Act
    let port = property(&object, "port");

    // Assert
    assert_eq!(slice(&port.range), "\"port\": 8080");
}

#[test]
fn value_ranges_cover_the_value_alone() {
    // Arrange
    let object = root();

    // Act
    let port = property(&object, "port");

    // Assert
    assert_eq!(slice(&port.value), "8080");
}

#[test]
fn object_ranges_cover_the_braces() {
    // Arrange
    let object = root();
    let tls = property(&object, "tls");

    // Act
    let Value::Object(inner) = &tls.value else {
        panic!("tls should be an object");
    };

    // Assert
    let text = slice(&inner.range);
    assert!(text.starts_with('{'), "got: {text:?}");
    assert!(text.ends_with('}'), "got: {text:?}");
    assert!(text.contains("\"cert\""), "got: {text:?}");
}

#[test]
fn array_elements_have_individual_ranges() {
    // Arrange
    let object = root();
    let allow = property(&object, "allow");

    // Act
    let Value::Array(array) = &allow.value else {
        panic!("allow should be an array");
    };

    // Assert
    let texts: Vec<&str> = array.elements.iter().map(slice).collect();
    assert_eq!(texts, vec!["\"10.0.0.0/8\"", "\"192.168.0.0/16\""]);
}

#[test]
fn ranges_past_multibyte_text_are_byte_indices() {
    // Arrange
    // The euro sign is three bytes and one character, so a char-counted range
    // would place `port` two bytes early.
    let object = root();

    // Act
    let port = property(&object, "port");

    // Assert
    assert_eq!(port.name.start(), INPUT.find("\"port\"").unwrap());
    assert!(INPUT.is_char_boundary(port.range.start));
    assert!(INPUT.is_char_boundary(port.range.end));
}

#[test]
fn string_values_carry_decoded_text_and_source_ranges() {
    // Arrange
    // The frontend stores the decoded value and the source range, so a string
    // holding escapes must keep the two independent.
    let input = "{\"greeting\": \"a\\nb\\u00e9\"}";

    // Act
    let parsed = parse_to_ast(input, &Default::default(), &strict()).unwrap();

    // Assert
    let Some(Value::Object(object)) = parsed.value else {
        panic!("the root should be an object");
    };
    let Value::StringLit(literal) = &object.properties[0].value else {
        panic!("greeting should be a string");
    };
    assert_eq!(literal.value.as_ref(), "a\nb\u{e9}");
    assert_eq!(
        &input[literal.range.start..literal.range.end],
        "\"a\\nb\\u00e9\""
    );
}

#[test]
fn number_values_carry_their_raw_text() {
    // Arrange
    // The frontend classifies a number by how it is written, which needs the source
    // text rather than a parsed value.
    let input = r#"{"whole": 4.0, "count": 4}"#;

    // Act
    let parsed = parse_to_ast(input, &Default::default(), &strict()).unwrap();

    // Assert
    let Some(Value::Object(object)) = parsed.value else {
        panic!("the root should be an object");
    };
    let raw: Vec<&str> = object
        .properties
        .iter()
        .map(|property| match &property.value {
            Value::NumberLit(literal) => literal.value,
            other => panic!("expected a number, got {other:?}"),
        })
        .collect();
    assert_eq!(raw, vec!["4.0", "4"]);
}

#[test]
fn duplicate_keys_survive_the_parse_in_order() {
    // Arrange
    // The duplicate-key mapping rests on both properties reaching the frontend,
    // which the property vector allows but nothing documents.
    let input = r#"{"allow": "a", "allow": "b"}"#;

    // Act
    let parsed = parse_to_ast(input, &Default::default(), &strict()).unwrap();

    // Assert
    let Some(Value::Object(object)) = parsed.value else {
        panic!("the root should be an object");
    };
    assert_eq!(object.properties.len(), 2);
    let values: Vec<&str> = object
        .properties
        .iter()
        .map(|property| match &property.value {
            Value::StringLit(literal) => literal.value.as_ref(),
            other => panic!("expected a string, got {other:?}"),
        })
        .collect();
    assert_eq!(values, vec!["a", "b"]);
}

#[test]
fn an_empty_document_parses_to_no_root_value() {
    // Arrange
    // The frontend's empty-document arm rests on this being a success with no
    // value rather than a parse error.
    let input = "  \n  ";

    // Act
    let parsed = parse_to_ast(input, &Default::default(), &strict()).unwrap();

    // Assert
    assert!(parsed.value.is_none());
}

#[test]
fn parse_error_ranges_are_byte_offsets() {
    // Arrange
    // The euro sign sits before the offending token, so a byte offset for the
    // missing comma is past it where a char count would fall short.
    let input = "{\"cost\": \"€\" \"port\": 8080}";

    // Act
    let Err(error) = parse_to_ast(input, &Default::default(), &strict()) else {
        panic!("a missing comma should be a parse error");
    };

    // Assert
    let range = error.range();
    assert!(
        input.is_char_boundary(range.start) && input.is_char_boundary(range.end),
        "range {}..{} is not on char boundaries",
        range.start,
        range.end
    );
    assert!(range.end <= input.len());
    assert_eq!(range.start, input.find("\u{20ac}").unwrap() + 4);
    assert_eq!(error.kind().to_string(), "Expected comma");
}
