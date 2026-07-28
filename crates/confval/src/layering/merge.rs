//! Deep merge of two neutral [`Fields`] levels.
//!
//! One rule covers the cross-product of value kinds. Two structural values,
//! whichever spelling each used, recurse and unify their entries by name. A
//! structural value against a non-structural one has no combined form and is
//! reported as a cross-source conflict. Two non-structural values follow the
//! value-level rule, where the higher-precedence side replaces the other.
//!
//! The merged level keeps the base layer's source and enclosing span, so a
//! missing-required-field error points at the base document rather than at an
//! override's one-line synthetic source.

use crate::diagnostic::Report;
use crate::format::{Field, FieldKind, Fields, Value, ValueKind};

/// How an incoming layer combines with the accumulated one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verb {
    /// The incoming layer replaces the accumulated one on overlap.
    Merge,
    /// The accumulated layer stands and the incoming one fills missing keys.
    Join,
}

/// Combines `incoming` into `base`, keeping `base`'s source and enclosing span.
pub(crate) fn combine(base: &Fields, incoming: &Fields, verb: Verb, report: &mut Report) -> Fields {
    let mut names: Vec<&str> = Vec::new();
    for field in base.iter().chain(incoming.iter()) {
        if !names.contains(&field.name.as_str()) {
            names.push(field.name.as_str());
        }
    }

    let mut items: Vec<Field> = Vec::new();
    for name in names {
        let base_group: Vec<&Field> = base.iter().filter(|f| f.name.as_str() == name).collect();
        let in_group: Vec<&Field> = incoming
            .iter()
            .filter(|f| f.name.as_str() == name)
            .collect();
        match (base_group.as_slice(), in_group.as_slice()) {
            (_, []) => items.extend(base_group.into_iter().cloned()),
            ([], _) => items.extend(in_group.into_iter().cloned()),
            ([acc], [inc]) => items.push(combine_field(acc, inc, verb, report)),
            // A repeated field is a nested list. The whole group replaces under
            // merge and stands under join, mirroring the array rule.
            (base_many, in_many) => match verb {
                Verb::Merge => items.extend(in_many.iter().copied().cloned()),
                Verb::Join => items.extend(base_many.iter().copied().cloned()),
            },
        }
    }

    Fields::new(base.source(), base.enclosing(), items)
}

fn combine_field(acc: &Field, incoming: &Field, verb: Verb, report: &mut Report) -> Field {
    match (structural_fields(acc), structural_fields(incoming)) {
        (Some(acc_inner), Some(in_inner)) => {
            let merged = combine(acc_inner, in_inner, verb, report);
            rewrap(acc, merged)
        }
        (Some(_), None) | (None, Some(_)) => {
            report_conflict(acc, incoming, report);
            keep(acc, incoming, verb)
        }
        (None, None) => keep(acc, incoming, verb),
    }
}

/// The inner level of a structural field, whichever spelling it used.
fn structural_fields(field: &Field) -> Option<&Fields> {
    match &field.kind {
        FieldKind::Block(fields) => Some(fields),
        FieldKind::Value(Value {
            kind: ValueKind::Map(fields),
            ..
        }) => Some(fields),
        _ => None,
    }
}

/// Wraps a merged inner level in the accumulator's spelling, so the result
/// reads as the base document with its overrides applied.
fn rewrap(acc: &Field, merged: Fields) -> Field {
    let kind = match &acc.kind {
        FieldKind::Block(_) => FieldKind::Block(merged),
        FieldKind::Value(value) => FieldKind::Value(Value {
            span: value.span,
            kind: ValueKind::Map(merged),
        }),
    };
    Field {
        kind,
        ..acc.clone()
    }
}

/// The incoming field wins under merge, the accumulated field under join.
fn keep(acc: &Field, incoming: &Field, verb: Verb) -> Field {
    match verb {
        Verb::Merge => incoming.clone(),
        Verb::Join => acc.clone(),
    }
}

fn report_conflict(acc: &Field, incoming: &Field, report: &mut Report) {
    let acc_label = kind_label(acc);
    let in_label = kind_label(incoming);
    report
        .error(format!(
            "`{}` is a {in_label} in one source and a {acc_label} in another",
            acc.name
        ))
        .at(incoming.span)
        .related(acc.span, format!("also defined here as a {acc_label}"))
        .emit();
}

fn kind_label(field: &Field) -> &'static str {
    match &field.kind {
        FieldKind::Block(_) => "block",
        FieldKind::Value(Value {
            kind: ValueKind::Map(_),
            ..
        }) => "object",
        FieldKind::Value(_) => "value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Scalar;
    use crate::source::{SourceId, Span};

    const A: SourceId = SourceId(0);
    const B: SourceId = SourceId(1);

    fn sp(source: SourceId, start: u32, end: u32) -> Span {
        Span::new(source, start, end)
    }

    fn scalar(name: &str, value: Scalar) -> Field {
        Field {
            name: name.to_string(),
            name_span: sp(A, 0, 0),
            span: sp(A, 0, 0),
            source: A,
            doc: None,
            kind: FieldKind::Value(Value {
                span: sp(A, 0, 0),
                kind: ValueKind::Scalar(value),
            }),
        }
    }

    fn seq(name: &str, values: Vec<Scalar>) -> Field {
        let elements = values
            .into_iter()
            .map(|value| Value {
                span: sp(A, 0, 0),
                kind: ValueKind::Scalar(value),
            })
            .collect();
        Field {
            name: name.to_string(),
            name_span: sp(A, 0, 0),
            span: sp(A, 0, 0),
            source: A,
            doc: None,
            kind: FieldKind::Value(Value {
                span: sp(A, 0, 0),
                kind: ValueKind::Seq(elements),
            }),
        }
    }

    fn block(name: &str, items: Vec<Field>) -> Field {
        Field {
            name: name.to_string(),
            name_span: sp(A, 0, 0),
            span: sp(A, 0, 0),
            source: A,
            doc: None,
            kind: FieldKind::Block(Fields::new(A, sp(A, 0, 0), items)),
        }
    }

    fn object(name: &str, items: Vec<Field>) -> Field {
        Field {
            name: name.to_string(),
            name_span: sp(A, 0, 0),
            span: sp(A, 0, 0),
            source: A,
            doc: None,
            kind: FieldKind::Value(Value {
                span: sp(A, 0, 0),
                kind: ValueKind::Map(Fields::new(A, sp(A, 0, 0), items)),
            }),
        }
    }

    fn level(source: SourceId, enclosing: Span, items: Vec<Field>) -> Fields {
        Fields::new(source, enclosing, items)
    }

    fn scalar_of(field: &Field) -> &Scalar {
        let FieldKind::Value(Value {
            kind: ValueKind::Scalar(scalar),
            ..
        }) = &field.kind
        else {
            panic!("expected a scalar field");
        };
        scalar
    }

    #[test]
    fn merge_overrides_a_scalar_with_the_higher_precedence_source() {
        // Arrange
        let base = level(A, sp(A, 0, 0), vec![scalar("port", Scalar::Int(1))]);
        let over = level(A, sp(A, 0, 0), vec![scalar("port", Scalar::Int(2))]);
        let mut report = Report::new();
        // Act
        let merged = combine(&base, &over, Verb::Merge, &mut report);
        // Assert
        assert_eq!(scalar_of(merged.get("port").unwrap()), &Scalar::Int(2));
        assert!(!report.has_issues());
    }

    #[test]
    fn join_keeps_the_base_scalar_and_fills_missing_keys() {
        // Arrange
        let base = level(A, sp(A, 0, 0), vec![scalar("port", Scalar::Int(1))]);
        let over = level(
            A,
            sp(A, 0, 0),
            vec![
                scalar("port", Scalar::Int(2)),
                scalar("host", Scalar::String("added".to_string())),
            ],
        );
        let mut report = Report::new();
        // Act
        let merged = combine(&base, &over, Verb::Join, &mut report);
        // Assert
        assert_eq!(scalar_of(merged.get("port").unwrap()), &Scalar::Int(1));
        assert_eq!(
            scalar_of(merged.get("host").unwrap()),
            &Scalar::String("added".to_string())
        );
    }

    #[test]
    fn merge_recurses_and_unifies_nested_blocks() {
        // Arrange
        let base = level(
            A,
            sp(A, 0, 0),
            vec![block("server", vec![scalar("port", Scalar::Int(1))])],
        );
        let over = level(
            A,
            sp(A, 0, 0),
            vec![block(
                "server",
                vec![scalar("host", Scalar::String("x".to_string()))],
            )],
        );
        let mut report = Report::new();
        // Act
        let merged = combine(&base, &over, Verb::Merge, &mut report);
        // Assert
        let FieldKind::Block(server) = &merged.get("server").unwrap().kind else {
            panic!("expected a block");
        };
        assert!(server.get("port").is_some());
        assert!(server.get("host").is_some());
        assert!(!report.has_issues());
    }

    #[test]
    fn merge_unifies_block_and_object_spellings_keeping_the_base_spelling() {
        // Arrange
        let base = level(
            A,
            sp(A, 0, 0),
            vec![block("server", vec![scalar("port", Scalar::Int(1))])],
        );
        let over = level(
            A,
            sp(A, 0, 0),
            vec![object(
                "server",
                vec![scalar("host", Scalar::String("x".to_string()))],
            )],
        );
        let mut report = Report::new();
        // Act
        let merged = combine(&base, &over, Verb::Merge, &mut report);
        // Assert
        let FieldKind::Block(server) = &merged.get("server").unwrap().kind else {
            panic!("expected the base block spelling to survive");
        };
        assert!(server.get("port").is_some());
        assert!(server.get("host").is_some());
    }

    #[test]
    fn merge_replaces_an_array_whole() {
        // Arrange
        let base = level(
            A,
            sp(A, 0, 0),
            vec![seq("allow", vec![Scalar::String("a".to_string())])],
        );
        let over = level(
            A,
            sp(A, 0, 0),
            vec![seq("allow", vec![Scalar::String("b".to_string())])],
        );
        let mut report = Report::new();
        // Act
        let merged = combine(&base, &over, Verb::Merge, &mut report);
        // Assert
        let FieldKind::Value(Value {
            kind: ValueKind::Seq(elements),
            ..
        }) = &merged.get("allow").unwrap().kind
        else {
            panic!("expected an array");
        };
        assert_eq!(elements.len(), 1);
    }

    #[test]
    fn scalar_against_block_reports_a_conflict_and_keeps_the_higher_precedence_side() {
        // Arrange
        let base = level(A, sp(A, 0, 0), vec![scalar("port", Scalar::Int(1))]);
        let over = level(A, sp(A, 0, 0), vec![block("port", vec![])]);
        let mut report = Report::new();
        // Act
        let merged = combine(&base, &over, Verb::Merge, &mut report);
        // Assert
        assert_eq!(
            report.issues()[0].message,
            "`port` is a block in one source and a value in another"
        );
        assert!(matches!(
            merged.get("port").unwrap().kind,
            FieldKind::Block(_)
        ));
    }

    #[test]
    fn merged_level_keeps_the_base_source_and_enclosing() {
        // Arrange
        let base = level(A, sp(A, 3, 7), vec![scalar("port", Scalar::Int(1))]);
        let over = level(B, sp(B, 9, 9), vec![scalar("host", Scalar::Int(2))]);
        let mut report = Report::new();
        // Act
        let merged = combine(&base, &over, Verb::Merge, &mut report);
        // Assert
        assert_eq!(merged.source(), A);
        assert_eq!(merged.enclosing(), sp(A, 3, 7));
    }
}
