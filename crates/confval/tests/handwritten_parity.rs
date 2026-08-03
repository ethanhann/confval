//! A handwritten impl built through `FieldsBuilder` emits what the derive
//! emits.
//!
//! One spec is written twice over the same fields, once with `#[derive(Spec)]`
//! and once by hand. Both parse the same document, and both walks are compared:
//! the rendered TOML, the field names, and the span attachment of every field
//! and every list element. Each row of the builder's semantics table is a claim
//! about generated code, so this is where the two are held to each other.

use confval::format::toml::{emit_toml, parse_toml};
use confval::format::{
    Field, FieldKind, Fields, FieldsBuilder, FromFields, ToFields, ValueKind, Walk,
    parse_int_field, parse_string_field, parse_string_list_field, parse_struct_field,
    parse_struct_list_field, report_missing_field, report_unknown_field,
};
use confval::prelude::*;

const DOCUMENT: &str = r#"name = "alpha"
port = 9090
tags = ["x", "y"]
allow = []

[limits]
size = 3

[[services]]
size = 4

[[services]]
"#;

#[derive(confval::Spec, Debug)]
#[confval(derive_default)]
struct Child {
    #[confval(default = 16)]
    size: Located<i64>,
}

impl Validate for Child {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, Debug)]
struct Derived {
    name: Located<String>,
    port: Option<Located<i64>>,
    #[confval(default)]
    tags: Vec<Located<String>>,
    allow: Option<Located<Vec<Located<String>>>>,
    #[confval(nested)]
    limits: Located<Child>,
    #[confval(nested)]
    extra: Option<Located<Child>>,
    #[confval(nested)]
    services: Vec<Located<Child>>,
}

impl Validate for Derived {
    fn validate(&self, _report: &mut Report) {}
}

/// The same fields, parsed and emitted by hand. The parse half uses the leaf
/// helpers and the emit half uses the builder, which is the pair a spec with a
/// shape the derive cannot express writes.
#[derive(Debug)]
struct Handwritten {
    name: Located<String>,
    port: Option<Located<i64>>,
    tags: Vec<Located<String>>,
    allow: Option<Located<Vec<Located<String>>>>,
    limits: Located<Child>,
    extra: Option<Located<Child>>,
    services: Vec<Located<Child>>,
}

impl FromFields for Handwritten {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self> {
        let mut name = None;
        let mut port = None;
        let mut tags = None;
        let mut allow = None;
        let mut limits = None;
        let mut extra = None;
        let mut services = Vec::new();

        for field in fields.iter() {
            match field.name.as_str() {
                "name" => name = parse_string_field(field, report),
                "port" => port = parse_int_field(field, report),
                "tags" => tags = parse_string_list_field(field, report),
                "allow" => allow = parse_string_list_field(field, report),
                "limits" => limits = parse_struct_field(field, report),
                "extra" => extra = parse_struct_field(field, report),
                "services" => parse_struct_list_field(&mut services, field, report),
                _ => report_unknown_field(field, report),
            }
        }

        if name.is_none() && !fields.has("name") {
            report_missing_field("name", fields.enclosing(), report);
        }
        if limits.is_none() && !fields.has("limits") {
            report_missing_field("limits", fields.enclosing(), report);
        }

        Some(Handwritten {
            name: name?,
            port,
            tags: tags.map(|list| list.value).unwrap_or_default(),
            allow,
            limits: limits?,
            extra,
            services,
        })
    }
}

impl Handwritten {
    fn build(&self, walk: Walk) -> Fields {
        FieldsBuilder::new(walk)
            .leaf("name", &self.name)
            .leaf_opt("port", self.port.as_ref())
            .string_list("tags", &self.tags)
            .string_list_opt("allow", self.allow.as_ref())
            .block("limits", &self.limits)
            .block_opt("extra", self.extra.as_ref())
            .block_list("services", &self.services)
            .finish()
    }
}

impl ToFields for Handwritten {
    fn to_fields(&self) -> Fields {
        self.build(Walk::Populated)
    }

    fn to_source_fields(&self) -> Fields {
        self.build(Walk::Source)
    }
}

fn parse_both() -> (Derived, Handwritten) {
    let mut sources = SourceMap::new();
    let id = sources.add("parity.toml", DOCUMENT.to_string());
    let mut report = Report::new();
    let derived = parse_toml::<Derived>(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("derived should parse: {:?}", report.issues()));
    let handwritten = parse_toml::<Handwritten>(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("handwritten should parse: {:?}", report.issues()));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    (derived, handwritten)
}

/// Every field and element location in one level, flattened depth first, so two
/// models compare as one list rather than field by field.
fn attachment(fields: &Fields) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for field in fields.iter() {
        out.push((field.name.clone(), !field.span.is_detached()));
        match &field.kind {
            FieldKind::Block(inner) => out.extend(attachment(inner)),
            FieldKind::Value(value) => {
                out.push((format!("{}.value", field.name), !value.span.is_detached()));
                if let ValueKind::Seq(elements) = &value.kind {
                    for (index, element) in elements.iter().enumerate() {
                        out.push((
                            format!("{}[{index}]", field.name),
                            !element.span.is_detached(),
                        ));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn the_populated_walks_render_the_same_text() {
    // Arrange
    let (derived, handwritten) = parse_both();

    // Act
    let from_derive = emit_toml(&derived.to_fields()).expect("emit derived");

    // Assert
    let by_hand = emit_toml(&handwritten.to_fields()).expect("emit handwritten");
    assert_eq!(from_derive, by_hand);
}

#[test]
fn the_source_walks_render_the_same_text() {
    // Arrange
    let (derived, handwritten) = parse_both();

    // Act
    let from_derive = emit_toml(&derived.to_source_fields()).expect("emit derived");

    // Assert
    let by_hand = emit_toml(&handwritten.to_source_fields()).expect("emit handwritten");
    assert_eq!(from_derive, by_hand);
}

#[test]
fn the_populated_walks_agree_on_every_span() {
    // Arrange
    let (derived, handwritten) = parse_both();

    // Act
    let from_derive = attachment(&derived.to_fields());

    // Assert
    let by_hand = attachment(&handwritten.to_fields());
    assert_eq!(from_derive, by_hand);
    assert!(
        from_derive.iter().all(|(_, attached)| !attached),
        "the populated walk detaches everything: {from_derive:?}"
    );
}

#[test]
fn the_source_walks_agree_on_every_span() {
    // Arrange
    let (derived, handwritten) = parse_both();

    // Act
    let from_derive = attachment(&derived.to_source_fields());

    // Assert
    let by_hand = attachment(&handwritten.to_source_fields());
    assert_eq!(from_derive, by_hand);
}

#[test]
fn the_source_walk_omits_what_the_document_left_out() {
    // Arrange
    // The document sets no `extra` block, and every `limits` and `services`
    // element that omits `size` leaves it defaulted.
    let (_, handwritten) = parse_both();

    // Act
    let source = emit_toml(&handwritten.to_source_fields()).expect("emit source");

    // Assert
    assert!(!source.contains("extra"), "source:\n{source}");
    assert!(source.contains("size = 3"), "source:\n{source}");
    assert!(source.contains("size = 4"), "source:\n{source}");
    assert_eq!(source.matches("size").count(), 2, "source:\n{source}");
    assert!(source.contains("allow = []"), "source:\n{source}");
}

#[test]
fn a_handwritten_source_walk_keeps_element_spans_the_derive_keeps() {
    // Arrange
    // `tags` is the bare list shape, whose elements carry the only locations it
    // has, so a walk that dropped them would still render the same text.
    let (derived, handwritten) = parse_both();
    let from_derive = derived.to_source_fields();

    // Act
    let by_hand = handwritten.to_source_fields();

    // Assert
    assert_eq!(element_spans(&from_derive, "tags"), vec![true, true]);
    assert_eq!(element_spans(&by_hand, "tags"), vec![true, true]);
    assert!(
        field_named(&by_hand, "tags").span.is_detached(),
        "the bare list itself has no location"
    );
}

fn field_named(fields: &Fields, name: &str) -> Field {
    fields
        .get(name)
        .unwrap_or_else(|| panic!("{name} should be present"))
        .clone()
}

fn element_spans(fields: &Fields, name: &str) -> Vec<bool> {
    match &field_named(fields, name).kind {
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Seq(elements) => elements
                .iter()
                .map(|element| !element.span.is_detached())
                .collect(),
            other => panic!("expected a sequence, got {other:?}"),
        },
        other => panic!("expected a value, got {other:?}"),
    }
}
