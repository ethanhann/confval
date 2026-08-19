//! Guards the span fidelity the `confval::format::kdl` adapter depends on. The
//! adapter needs kdl-rs to expose byte-accurate spans for node names, argument
//! entries, property entries, nodes, and children documents, plus byte offsets
//! on syntax diagnostics. If a kdl upgrade breaks any of these, error
//! attribution breaks with it.

#![allow(clippy::unwrap_used, clippy::expect_used)]
use kdl::KdlDocument;

const INPUT: &str = r#"server {
  hostname "example.com"
  port 8080
  daemon #true

  tls cert="cert.pem" {
    key "key.pem"
  }

  allow "10.0.0.0/8" "not a cidr"
}
"#;

fn parse() -> KdlDocument {
    KdlDocument::parse_v2(INPUT).unwrap()
}

/// Slices `INPUT` at a kdl-rs span. A macro rather than a function, because
/// the span type belongs to miette, which is not a direct dependency.
macro_rules! slice {
    ($span:expr) => {{
        let span = $span;
        &INPUT[span.offset()..span.offset() + span.len()]
    }};
}

#[test]
fn node_name_spans_are_byte_accurate() {
    // Arrange
    let document = parse();

    // Act
    let server = document.nodes().first().unwrap();

    // Assert
    assert_eq!(slice!(server.name().span()), "server");
}

#[test]
fn argument_entry_spans_cover_the_value_text() {
    // Arrange
    let document = parse();
    let server = document.nodes().first().unwrap();
    let children = server.children().unwrap();

    // Act
    let hostname = children
        .nodes()
        .iter()
        .find(|node| node.name().value() == "hostname")
        .unwrap();

    // Assert
    assert_eq!(slice!(hostname.entries()[0].span()), "\"example.com\"");
}

#[test]
fn property_entry_spans_cover_name_and_value_together() {
    // Arrange
    // kdl-rs spans a property from its name through its value and gives the
    // value no span of its own. The frontend narrows it to the value using the
    // entry's `value_repr`, so this guards the two inputs that arithmetic reads.
    let document = parse();
    let server = document.nodes().first().unwrap();
    let children = server.children().unwrap();

    // Act
    let tls = children
        .nodes()
        .iter()
        .find(|node| node.name().value() == "tls")
        .unwrap();

    // Assert
    let cert = &tls.entries()[0];
    assert_eq!(slice!(cert.span()), "cert=\"cert.pem\"");
    assert_eq!(slice!(cert.name().unwrap().span()), "cert");
    // The value's text is the tail of the entry span, which is what lets the
    // frontend locate it without the source.
    assert_eq!(cert.format().unwrap().value_repr, "\"cert.pem\"");
}

#[test]
fn list_arguments_have_individual_entry_spans() {
    // Arrange
    let document = parse();
    let server = document.nodes().first().unwrap();
    let children = server.children().unwrap();

    // Act
    let allow = children
        .nodes()
        .iter()
        .find(|node| node.name().value() == "allow")
        .unwrap();

    // Assert
    let texts: Vec<&str> = allow
        .entries()
        .iter()
        .map(|entry| slice!(entry.span()))
        .collect();
    assert_eq!(texts, vec!["\"10.0.0.0/8\"", "\"not a cidr\""]);
}

#[test]
fn node_spans_cover_the_whole_node() {
    // Arrange
    let document = parse();

    // Act
    let server = document.nodes().first().unwrap();

    // Assert
    let text = slice!(server.span());
    assert!(text.starts_with("server {"), "got: {text:?}");
    assert!(text.trim_end().ends_with('}'), "got: {text:?}");
}

#[test]
fn children_document_spans_sit_inside_the_braces() {
    // Arrange
    let document = parse();
    let server = document.nodes().first().unwrap();

    // Act
    let children = server.children().unwrap();

    // Assert
    let span = children.span();
    let open = INPUT.find('{').unwrap();
    let close = INPUT.rfind('}').unwrap();
    assert!(
        span.offset() > open,
        "children span starts at {}",
        span.offset()
    );
    assert!(span.offset() + span.len() <= close + 1);
}

#[test]
fn diagnostic_spans_are_byte_offsets() {
    // Arrange
    // The three-byte euro sign sits before the offending token, so a byte
    // offset for the `=` is 16 where a char count would say 14. The
    // diagnostic span field's own documentation says chars while the parser
    // emits bytes, and pinning the exact offset makes a kdl upgrade that
    // resolves the discrepancy the other way fail loudly.
    let input = "cost \"€\"\nport = 8080\n";

    // Act
    let error = KdlDocument::parse_v2(input).unwrap_err();

    // Assert
    let first = &error.diagnostics[0];
    assert_eq!(
        first.span.offset(),
        16,
        "diagnostics: {:?}",
        error.diagnostics
    );
    assert_eq!(
        &input[first.span.offset()..first.span.offset() + first.span.len()],
        "="
    );
    for diagnostic in &error.diagnostics {
        let start = diagnostic.span.offset();
        let end = start + diagnostic.span.len();
        assert!(end <= input.len(), "span {start}..{end} exceeds the input");
        assert!(
            input.is_char_boundary(start) && input.is_char_boundary(end),
            "span {start}..{end} is not on byte-accurate char boundaries"
        );
    }
}
