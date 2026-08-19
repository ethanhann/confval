//! Guards the shape-decided resolution of repeated value fields: a list-shaped
//! field accumulates same-named occurrences, and a single-value field reports
//! the second occurrence as a duplicate pointing back at the first. The
//! resolution is in the generated walk, so a hand-built `Fields` exercises
//! it with no frontend in the loop, pinning the behavior for every format.

use confval::format::{Entry, Field, FieldKind, Fields, FromFields, Scalar, Value, ValueKind};
use confval::prelude::*;
use confval::source::SourceId;

/// A registered source id for hand-built fields. The text is never rendered,
/// so its content does not matter.
fn source() -> SourceId {
    let mut sources = SourceMap::new();
    sources.add("hand.built", "")
}

fn span(source: SourceId, start: u32, end: u32) -> Span {
    Span::new(source, start, end)
}

fn scalar_field(source: SourceId, name: &str, scalar: Scalar, at: u32) -> Field {
    Field::parsed(
        name,
        span(source, at, at + name.len() as u32),
        span(source, at, at + 10),
        source,
        FieldKind::Value(Value {
            span: span(source, at, at + 10),
            kind: ValueKind::Scalar(scalar),
        }),
    )
}

fn seq_field(source: SourceId, name: &str, elements: Vec<Scalar>, at: u32) -> Field {
    let values = elements
        .into_iter()
        .map(|scalar| Value {
            span: span(source, at, at + 1),
            kind: ValueKind::Scalar(scalar),
        })
        .collect();
    Field::parsed(
        name,
        span(source, at, at + name.len() as u32),
        span(source, at, at + 10),
        source,
        FieldKind::Value(Value {
            span: span(source, at, at + 10),
            kind: ValueKind::Seq(values),
        }),
    )
}

fn level(source: SourceId, items: Vec<Field>) -> Fields {
    Fields::new(source, span(source, 0, 100), items)
}

fn entry_level(source: SourceId, items: Vec<Entry>) -> Fields {
    Fields::from_entries(source, span(source, 0, 100), items)
}

#[derive(confval::Spec, PartialEq, Debug)]
struct SingleSpec {
    port: Located<i64>,
}

impl Validate for SingleSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct DefaultedSpec {
    #[confval(default = 4)]
    workers: Located<i64>,
}

impl Validate for DefaultedSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct BareListSpec {
    allow: Vec<Located<String>>,
}

impl Validate for BareListSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct WrappedListSpec {
    allow: Option<Located<Vec<Located<String>>>>,
}

impl Validate for WrappedListSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_commented_field_reads_as_absent_to_the_walk() {
    // Arrange
    let source = source();
    let fields = entry_level(
        source,
        vec![scalar_field(source, "port", Scalar::Int(9090), 0).as_commented()],
    );
    let mut report = Report::new();

    // Act
    let spec = SingleSpec::from_fields(&fields, &mut report);

    // Assert
    // The commented placeholder never activates the field, so the level reads
    // as missing the required value.
    assert!(spec.is_none());
    assert_eq!(report.issues()[0].message, "missing required field: port");
}

#[test]
fn a_commented_unknown_name_reports_nothing() {
    // Arrange
    let source = source();
    let fields = entry_level(
        source,
        vec![
            scalar_field(source, "port", Scalar::Int(8080), 0).into(),
            scalar_field(source, "hostnme", Scalar::Int(1), 20).as_commented(),
        ],
    );
    let mut report = Report::new();

    // Act
    let spec = SingleSpec::from_fields(&fields, &mut report);

    // Assert
    assert_eq!(spec.expect("the active field parses").port.value, 8080);
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
}

#[test]
fn a_repeated_single_value_field_reports_a_duplicate_and_keeps_the_first() {
    // Arrange
    let source = source();
    let fields = level(
        source,
        vec![
            scalar_field(source, "port", Scalar::Int(8080), 0),
            scalar_field(source, "port", Scalar::Int(9090), 20),
        ],
    );
    let mut report = Report::new();

    // Act
    let spec = SingleSpec::from_fields(&fields, &mut report);

    // Assert
    let spec = spec.expect("first occurrence should win");
    assert_eq!(spec.port.value, 8080);
    assert_eq!(report.issues().len(), 1);
    assert_eq!(report.issues()[0].message, "duplicate field: port");
    assert_eq!(report.issues()[0].span, Some(span(source, 20, 30)));
    assert_eq!(
        report.issues()[0].related[0],
        (span(source, 0, 10), "first declared here".to_string())
    );
}

#[test]
fn a_repeated_defaulted_field_reports_a_duplicate_and_keeps_the_first() {
    // Arrange
    let source = source();
    let fields = level(
        source,
        vec![
            scalar_field(source, "workers", Scalar::Int(2), 0),
            scalar_field(source, "workers", Scalar::Int(8), 20),
        ],
    );
    let mut report = Report::new();

    // Act
    let spec = DefaultedSpec::from_fields(&fields, &mut report);

    // Assert
    assert_eq!(spec.expect("first occurrence should win").workers.value, 2);
    assert_eq!(report.issues().len(), 1);
    assert_eq!(report.issues()[0].message, "duplicate field: workers");
}

#[test]
fn a_repeated_bare_list_field_accumulates_in_document_order() {
    // Arrange
    // The second occurrence is a lone scalar, so this also exercises the
    // widened one-element form inside an accumulation.
    let source = source();
    let fields = level(
        source,
        vec![
            seq_field(source, "allow", vec![Scalar::String("a".to_string())], 0),
            scalar_field(source, "allow", Scalar::String("b".to_string()), 20),
        ],
    );
    let mut report = Report::new();

    // Act
    let spec = BareListSpec::from_fields(&fields, &mut report);

    // Assert
    let spec = spec.expect("occurrences should accumulate");
    let allow: Vec<&str> = spec
        .allow
        .iter()
        .map(|element| element.value.as_str())
        .collect();
    assert_eq!(allow, vec!["a", "b"]);
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
}

#[test]
fn a_repeated_wrapped_list_field_accumulates_in_document_order() {
    // Arrange
    let source = source();
    let fields = level(
        source,
        vec![
            seq_field(source, "allow", vec![Scalar::String("a".to_string())], 0),
            seq_field(
                source,
                "allow",
                vec![
                    Scalar::String("b".to_string()),
                    Scalar::String("c".to_string()),
                ],
                20,
            ),
        ],
    );
    let mut report = Report::new();

    // Act
    let spec = WrappedListSpec::from_fields(&fields, &mut report);

    // Assert
    let spec = spec.expect("occurrences should accumulate");
    let list = spec.allow.expect("the list should be present");
    let allow: Vec<&str> = list
        .value
        .iter()
        .map(|element| element.value.as_str())
        .collect();
    assert_eq!(allow, vec!["a", "b", "c"]);
    // The stored span stays the first occurrence's.
    assert_eq!(list.span, span(source, 0, 10));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
}
