//! Guards the span fidelity the `confval::format::yaml` adapter depends on. The
//! adapter needs saphyr-parser to carry a position on every event, to count
//! those positions in characters, and to decode a scalar's text before handing
//! it over. If a saphyr-parser upgrade changes the event API, the offset unit,
//! or the decoding, error attribution breaks with it.
//!
//! The offset unit is the one to watch. `Marker`'s own field documents a
//! character index and its `index` accessor documents a byte index, and the two
//! disagree. The frontend converts, so a change in either direction fails here
//! rather than silently misplacing every span past a multibyte character.

#![allow(clippy::unwrap_used, clippy::expect_used)]
use saphyr_parser::{Event, Parser, ScalarStyle, Span};

/// The euro sign ahead of every later entry makes a character count disagree
/// with a byte offset by two from `port` onward.
const INPUT: &str = r#"cost: "€"
port: 8080
tls:
  cert: "cert.pem"
allow: ["10.0.0.0/8", "192.168.0.0/16"]
"#;

/// Every event in one document, paired with its span.
fn events(text: &str) -> Vec<(Event<'_>, Span)> {
    let mut parser = Parser::new_from_str(text);
    let mut out = Vec::new();
    while let Some(step) = parser.next_event() {
        let step = step.expect("the fixture parses");
        let done = matches!(step.0, Event::StreamEnd);
        out.push(step);
        if done {
            break;
        }
    }
    out
}

/// The nth scalar event, which is how the frontend reaches keys and values.
fn scalar(text: &str, nth: usize) -> (String, ScalarStyle, Span) {
    events(text)
        .into_iter()
        .filter_map(|(event, span)| match event {
            Event::Scalar(value, style, ..) => Some((value.into_owned(), style, span)),
            _ => None,
        })
        .nth(nth)
        .expect("the fixture has that many scalars")
}

#[test]
fn scalar_positions_are_character_indices() {
    // Arrange
    // The euro sign is three bytes and one character. `port` is at byte 12
    // and character 10, so this assertion is what tells the two apart.
    let byte = INPUT.find("port").unwrap();

    // Act
    let (text, _, span) = scalar(INPUT, 2);

    // Assert
    assert_eq!(text, "port");
    assert_eq!(byte, 12, "the fixture must put a multibyte character first");
    assert_eq!(
        span.start.index(),
        10,
        "positions are character indices, so the frontend must convert"
    );
}

#[test]
fn a_key_span_covers_the_key_alone() {
    // Arrange
    let text = "port: 8080\n";

    // Act
    let (value, _, span) = scalar(text, 0);

    // Assert
    assert_eq!(value, "port");
    assert_eq!(&text[span.start.index()..span.end.index()], "port");
}

#[test]
fn a_value_span_covers_the_value_alone() {
    // Arrange
    let text = "port: 8080\n";

    // Act
    let (value, _, span) = scalar(text, 1);

    // Assert
    assert_eq!(value, "8080");
    assert_eq!(&text[span.start.index()..span.end.index()], "8080");
}

#[test]
fn a_block_mapping_runs_from_its_first_entry_to_its_end() {
    // Arrange
    // A block mapping has no closing bracket, so the frontend derives a nested
    // level's enclosing span from the opening and closing events.
    let text = "tls:\n  cert: a.pem\n  key: k.pem\nport: 1\n";
    let stream = events(text);

    // Act
    let opened = stream
        .iter()
        .filter(|(event, _)| matches!(event, Event::MappingStart(..)))
        .nth(1)
        .expect("the nested mapping opens");
    let closed = stream
        .iter()
        .find(|(event, _)| matches!(event, Event::MappingEnd))
        .expect("the nested mapping closes");

    // Assert
    let inner = &text[opened.1.start.index()..closed.1.end.index()];
    assert!(inner.starts_with("cert"), "got: {inner:?}");
    assert!(inner.contains("k.pem"), "got: {inner:?}");
}

#[test]
fn a_valueless_key_yields_a_zero_width_null() {
    // Arrange
    // `key:` reads as a null whose scalar has no extent, which is why the
    // frontend widens a zero-width span to one position.
    let text = "key:\nport: 1\n";

    // Act
    let (value, _, span) = scalar(text, 1);

    // Assert
    assert_eq!(value, "~");
    assert_eq!(span.start.index(), span.end.index());
}

#[test]
fn scalar_text_arrives_decoded() {
    // Arrange
    // The span covers the raw text and the value carries the decoded one, so
    // the frontend can hold both without doing the decoding itself.
    let text = "greeting: \"a\\nb\\u00e9\"\n";

    // Act
    let (value, style, span) = scalar(text, 1);

    // Assert
    assert_eq!(value, "a\nb\u{e9}");
    assert_eq!(style, ScalarStyle::DoubleQuoted);
    assert_eq!(
        &text[span.start.index()..span.end.index()],
        "\"a\\nb\\u00e9\""
    );
}

#[test]
fn a_literal_block_arrives_folded() {
    // Arrange
    let text = "text: |\n  line one\n  line two\n";

    // Act
    let (value, style, _) = scalar(text, 1);

    // Assert
    assert_eq!(value, "line one\nline two\n");
    assert_eq!(style, ScalarStyle::Literal);
}

#[test]
fn a_plain_scalar_carries_its_style_so_the_schema_can_run() {
    // Arrange
    // Only a plain scalar resolves through the core schema, so the style is
    // what separates `port: 8080` from `port: "8080"`.
    let text = "a: 8080\nb: \"8080\"\nc: '8080'\n";

    // Act
    let styles: Vec<ScalarStyle> = (0..6).map(|nth| scalar(text, nth).1).collect();

    // Assert
    assert_eq!(styles[1], ScalarStyle::Plain);
    assert_eq!(styles[3], ScalarStyle::DoubleQuoted);
    assert_eq!(styles[5], ScalarStyle::SingleQuoted);
}

#[test]
fn duplicate_keys_survive_the_parse_in_order() {
    // Arrange
    // The duplicate-key mapping rests on the stream handing over both entries.
    // A document loader would have collapsed them into a map before this.
    let text = "allow: a\nallow: b\n";

    // Act
    let values: Vec<String> = (0..4).map(|nth| scalar(text, nth).0).collect();

    // Assert
    assert_eq!(values, vec!["allow", "a", "allow", "b"]);
}

#[test]
fn a_second_document_opens_its_own_event() {
    // Arrange
    // The frontend refuses a second document, which needs the event to be
    // visible rather than folded into the first.
    let text = "a: 1\n---\nb: 2\n";

    // Act
    let starts = events(text)
        .iter()
        .filter(|(event, _)| matches!(event, Event::DocumentStart(_)))
        .count();

    // Assert
    assert_eq!(starts, 2);
}

#[test]
fn a_scan_error_carries_a_position_and_a_lowercase_message() {
    // Arrange
    // A tab in block indentation is the parser's judgment. The frontend
    // prefixes the message with `syntax error: `, so its case matters.
    let text = "a:\n\tb: 1\n";
    let mut parser = Parser::new_from_str(text);

    // Act
    let error = loop {
        match parser.next_event() {
            Some(Ok(_)) => continue,
            Some(Err(error)) => break error,
            None => panic!("the fixture must fail to scan"),
        }
    };

    // Assert
    assert!(error.marker().index() <= text.chars().count());
    assert!(!error.info().is_empty());
    assert!(
        error.info().starts_with(|first: char| first.is_lowercase()),
        "got: {}",
        error.info()
    );
}
