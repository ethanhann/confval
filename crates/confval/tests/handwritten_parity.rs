//! A handwritten impl built through `FieldsBuilder` emits what the derive
//! emits.
//!
//! One spec is written twice over the same fields, once with `#[derive(Spec)]`
//! and once by hand. Both parse the same document.
//! The comparison covers the rendered TOML, the field names, and the span of
//! every field, value, list element, and map entry. Each row of the builder's
//! semantics table is a claim about generated code, so this test checks every
//! row against what the derive emits. The `headers` map is a shape the derive
//! expresses but the builder has no method for, so the handwritten side builds
//! and pushes it, and its parity is checked the same way.

use confval::format::toml::{emit_toml, parse_toml};
use confval::format::{
    Field, FieldKind, Fields, FieldsBuilder, FromFields, Scalar, ToFields, Value, ValueKind, Walk,
    parse_int_field, parse_string_field, parse_string_list_field, parse_string_map_field,
    parse_struct_field, parse_struct_list_field, report_missing_field, report_unknown_field,
};
use confval::prelude::*;
use std::collections::BTreeMap;

const DOCUMENT: &str = r#"name = "alpha"
port = 9090
tags = ["x", "y"]
allow = []
headers = { a = "1", b = "2" }

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
    #[confval(map, default)]
    headers: BTreeMap<String, Located<String>>,
    #[confval(nested)]
    limits: Located<Child>,
    #[confval(nested)]
    extra: Option<Located<Child>>,
    // The populate marker: an absent block is filled from `Child::default()` on
    // the populated walk and omitted from the source view.
    #[confval(nested, default)]
    fallback: Option<Located<Child>>,
    #[confval(nested)]
    services: Vec<Located<Child>>,
}

impl Validate for Derived {
    fn validate(&self, _report: &mut Report) {}
}

/// The same fields, parsed and emitted by hand. The parse half uses the leaf
/// helpers and the emit half uses the builder. A handwritten spec writes that
/// pair, whether for a shape the derive cannot express or for a map the builder
/// has no method for.
#[derive(Debug)]
struct Handwritten {
    name: Located<String>,
    port: Option<Located<i64>>,
    tags: Vec<Located<String>>,
    allow: Option<Located<Vec<Located<String>>>>,
    headers: BTreeMap<String, Located<String>>,
    limits: Located<Child>,
    extra: Option<Located<Child>>,
    fallback: Option<Located<Child>>,
    services: Vec<Located<Child>>,
}

impl FromFields for Handwritten {
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self> {
        let mut name = None;
        let mut port = None;
        let mut tags = None;
        let mut allow = None;
        let mut headers = None;
        let mut limits = None;
        let mut extra = None;
        let mut fallback = None;
        let mut services = Vec::new();

        for field in fields.iter() {
            match field.name.as_str() {
                "name" => name = parse_string_field(field, report),
                "port" => port = parse_int_field(field, report),
                "tags" => tags = parse_string_list_field(field, report),
                "allow" => allow = parse_string_list_field(field, report),
                "headers" => headers = parse_string_map_field(field, report),
                "limits" => limits = parse_struct_field(field, report),
                "extra" => extra = parse_struct_field(field, report),
                "fallback" => fallback = parse_struct_field(field, report),
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
            headers: headers.map(|map| map.value).unwrap_or_default(),
            limits: limits?,
            extra,
            fallback,
            services,
        })
    }
}

impl Handwritten {
    fn build(&self, walk: Walk) -> Fields {
        let mut builder = FieldsBuilder::new(walk)
            .leaf("name", &self.name)
            .leaf_opt("port", self.port.as_ref())
            .string_list("tags", &self.tags)
            .string_list_opt("allow", self.allow.as_ref());
        // The builder shapes no map, so the map field is built by hand and
        // pushed in the field's declaration position, where the derive emits it.
        if let Some(field) = string_map_field(walk, "headers", &self.headers) {
            builder = builder.push(field);
        }
        builder
            .block("limits", &self.limits)
            .block_opt("extra", self.extra.as_ref())
            .block_opt_default("fallback", self.fallback.as_ref())
            .block_list("services", &self.services)
            .finish()
    }
}

/// Builds a `headers` map field for one walk, the shape the derive's `Map` arm
/// emits. The populated walk emits every entry detached, and the source walk
/// keeps only source-written entries and drops the field when none remain.
fn string_map_field(
    walk: Walk,
    name: &str,
    map: &BTreeMap<String, Located<String>>,
) -> Option<Field> {
    let source = matches!(walk, Walk::Source);
    let mut entries = Vec::new();
    for (key, value) in map {
        if source && value.span.is_detached() {
            continue;
        }
        let scalar = ValueKind::Scalar(Scalar::String(value.value.clone()));
        let inner = if source {
            Value::spanned(value.span, scalar)
        } else {
            Value::detached(scalar)
        };
        entries.push(Field::detached_value(key, inner));
    }
    if source && entries.is_empty() {
        return None;
    }
    Some(Field::detached_value(
        name,
        Value::detached(ValueKind::Map(Fields::detached(entries))),
    ))
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
///
/// The span itself is recorded rather than whether it is attached. Both models
/// read one `SourceMap`, so a walk that had the wrong location would differ
/// here while a boolean would not notice.
fn spans(fields: &Fields) -> Vec<(String, Span)> {
    let mut out = Vec::new();
    for field in fields.iter() {
        out.push((field.name.clone(), field.span));
        match &field.kind {
            FieldKind::Block(inner) => out.extend(spans(inner)),
            FieldKind::Value(value) => {
                out.push((format!("{}.value", field.name), value.span));
                match &value.kind {
                    ValueKind::Seq(elements) => {
                        for (index, element) in elements.iter().enumerate() {
                            out.push((format!("{}[{index}]", field.name), element.span));
                        }
                    }
                    // A map value's entries hold the only per-key locations it
                    // has, so the parity tests descend into them too.
                    ValueKind::Map(inner) => out.extend(spans(inner)),
                    _ => {}
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
    let from_derive = spans(&derived.to_fields());

    // Assert
    let by_hand = spans(&handwritten.to_fields());
    assert_eq!(from_derive, by_hand);
    assert!(
        from_derive.iter().all(|(_, span)| span.is_detached()),
        "the populated walk detaches everything: {from_derive:?}"
    );
}

#[test]
fn the_source_walks_agree_on_every_span() {
    // Arrange
    let (derived, handwritten) = parse_both();

    // Act
    let from_derive = spans(&derived.to_source_fields());

    // Assert
    let by_hand = spans(&handwritten.to_source_fields());
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
    // `tags` is the bare list shape, whose elements hold the only locations it
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
