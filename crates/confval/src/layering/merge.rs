//! Deep merge of two neutral [`Fields`] levels.
//!
//! One rule covers the cross-product of value kinds. Two structural values,
//! whichever form each used, recurse and unify their entries by name. A
//! structural value against a non-structural one has no combined form and is
//! reported as a cross-source conflict. Two non-structural values follow the
//! value-level rule, where the higher-precedence side replaces the other.
//!
//! The merged level keeps the base layer's source and enclosing span, so a
//! missing-required-field error points at the base document rather than at an
//! override's one-line synthetic source.

use crate::diagnostic::Report;
use crate::format::{Field, FieldKind, Fields, Value, ValueKind};
use crate::source::{SourceId, Span};

/// How an incoming layer combines with the accumulated one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verb {
    /// The incoming layer replaces the accumulated one on overlap.
    Merge,
    /// The accumulated layer stands and the incoming one fills missing keys.
    Join,
}

/// Combines `incoming` into `base`, keeping `base`'s source and enclosing span.
/// Both levels are taken by value, so a kept field moves rather than clones.
pub(crate) fn combine(base: Fields, incoming: Fields, verb: Verb, report: &mut Report) -> Fields {
    let source = base.source();
    let enclosing = base.enclosing();
    let mut incoming_groups = grouped(incoming);
    let mut items: Vec<Field> = Vec::new();
    for (name, mut base_group) in grouped(base) {
        let position = incoming_groups.iter().position(|(other, _)| *other == name);
        let Some(mut in_group) = position.map(|index| incoming_groups.remove(index).1) else {
            items.append(&mut base_group);
            continue;
        };
        if base_group.len() == 1
            && in_group.len() == 1
            && let (Some(acc), Some(inc)) = (base_group.pop(), in_group.pop())
        {
            items.push(combine_field(acc, inc, verb, report));
            continue;
        }
        // A repeated field is a nested list. The whole group replaces under
        // merge and stands under join, mirroring the array rule. A kind
        // mismatch between the groups is the same cross-source conflict the
        // one-to-one arm reports, judged by each group's first field since a
        // parsed group is homogeneous.
        if let (Some(acc), Some(inc)) = (base_group.first(), in_group.first())
            && structural_fields(acc).is_some() != structural_fields(inc).is_some()
        {
            report_conflict(acc, inc, report);
        }
        match verb {
            Verb::Merge => items.append(&mut in_group),
            Verb::Join => items.append(&mut base_group),
        }
    }
    for (_, mut group) in incoming_groups {
        items.append(&mut group);
    }
    Fields::new(source, enclosing, items)
}

/// Partitions a level into same-named groups, keeping first-appearance order
/// within and across groups.
///
/// `into_items` yields the active fields alone, so a commented entry at this
/// level joins no group and conflicts with nothing. The nested levels need
/// [`without_commented`], because a repeated-block group is appended whole
/// rather than merged, so nothing else would reach inside it.
fn grouped(fields: Fields) -> Vec<(String, Vec<Field>)> {
    let mut groups: Vec<(String, Vec<Field>)> = Vec::new();
    for field in fields.into_items().into_iter().map(without_commented) {
        match groups.iter_mut().find(|(name, _)| *name == field.name) {
            Some((_, group)) => group.push(field),
            None => {
                let name = field.name.clone();
                groups.push((name, vec![field]));
            }
        }
    }
    groups
}

/// Drops commented entries from every level inside `field`, through blocks,
/// maps, and sequence elements.
///
/// `Fields::into_items` drops them one level at a time, which covers every
/// level the merge itself recurses into. A repeated-block group is appended
/// without recursing, so its inner levels reach assembled output only through
/// this walk.
fn without_commented(mut field: Field) -> Field {
    fn strip_fields(fields: Fields) -> Fields {
        let source = fields.source();
        let enclosing = fields.enclosing();
        let items = fields
            .into_items()
            .into_iter()
            .map(without_commented)
            .collect();
        Fields::new(source, enclosing, items)
    }
    fn strip_value(value: &mut Value) {
        match &mut value.kind {
            ValueKind::Map(inner) => {
                let taken = std::mem::replace(
                    inner,
                    Fields::new(SourceId::DETACHED, Span::detached(), Vec::new()),
                );
                *inner = strip_fields(taken);
            }
            ValueKind::Seq(elements) => {
                for element in elements {
                    strip_value(element);
                }
            }
            ValueKind::Scalar(_) | ValueKind::Other(_) => {}
        }
    }
    match &mut field.kind {
        FieldKind::Block(inner) => {
            let taken = std::mem::replace(
                inner,
                Fields::new(SourceId::DETACHED, Span::detached(), Vec::new()),
            );
            *inner = strip_fields(taken);
        }
        FieldKind::Value(value) => strip_value(value),
    }
    field
}

fn combine_field(acc: Field, incoming: Field, verb: Verb, report: &mut Report) -> Field {
    use Split::{Plain, Structural};
    match (split_structural(acc), split_structural(incoming)) {
        (Structural(shell, acc_inner), Structural(_, in_inner)) => {
            let merged = combine(acc_inner, in_inner, verb, report);
            shell.rewrap(merged)
        }
        (Structural(shell, acc_inner), Plain(incoming)) => {
            let acc = shell.rewrap(acc_inner);
            report_conflict(&acc, &incoming, report);
            keep(acc, incoming, verb)
        }
        (Plain(acc), Structural(shell, in_inner)) => {
            let incoming = shell.rewrap(in_inner);
            report_conflict(&acc, &incoming, report);
            keep(acc, incoming, verb)
        }
        (Plain(acc), Plain(incoming)) => keep(acc, incoming, verb),
    }
}

/// The structural form a field used, so a merged inner level can be
/// wrapped back in the form the base document had.
enum Nesting {
    Block,
    Map { value_span: Span },
}

/// A structural field taken apart: everything but the inner level.
struct Shell {
    name: String,
    name_span: Span,
    span: Span,
    source: SourceId,
    doc: Option<String>,
    nesting: Nesting,
}

impl Shell {
    fn rewrap(self, inner: Fields) -> Field {
        let kind = match self.nesting {
            Nesting::Block => FieldKind::Block(inner),
            Nesting::Map { value_span } => FieldKind::Value(Value {
                span: value_span,
                kind: ValueKind::Map(inner),
            }),
        };
        Field {
            name: self.name,
            name_span: self.name_span,
            span: self.span,
            source: self.source,
            doc: self.doc,
            kind,
        }
    }
}

/// A field taken apart for the merge. A structural field splits into its shell
/// and inner level. Any other field passes through whole.
enum Split {
    Structural(Shell, Fields),
    Plain(Field),
}

fn split_structural(field: Field) -> Split {
    let Field {
        name,
        name_span,
        span,
        source,
        doc,
        kind,
    } = field;
    let (nesting, inner) = match kind {
        FieldKind::Block(inner) => (Nesting::Block, inner),
        FieldKind::Value(Value {
            kind: ValueKind::Map(inner),
            span: value_span,
        }) => (Nesting::Map { value_span }, inner),
        kind => {
            return Split::Plain(Field {
                name,
                name_span,
                span,
                source,
                doc,
                kind,
            });
        }
    };
    Split::Structural(
        Shell {
            name,
            name_span,
            span,
            source,
            doc,
            nesting,
        },
        inner,
    )
}

/// The inner level of a structural field, whichever form it used.
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

/// The incoming field wins under merge, the accumulated field under join. The
/// doc comes from the accumulated side either way, matching [`Shell::rewrap`],
/// so a template's comment survives a value override.
fn keep(acc: Field, incoming: Field, verb: Verb) -> Field {
    match verb {
        Verb::Merge => Field {
            doc: acc.doc,
            ..incoming
        },
        Verb::Join => acc,
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
    use crate::format::{Entry, Scalar};
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

    fn entry_level(source: SourceId, enclosing: Span, items: Vec<Entry>) -> Fields {
        Fields::from_entries(source, enclosing, items)
    }

    fn entry_block(name: &str, items: Vec<Entry>) -> Field {
        Field::detached_block(name, Fields::from_entries(A, sp(A, 0, 0), items))
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
    fn a_repeated_object_group_against_a_repeated_block_group_is_no_conflict() {
        // Arrange
        // A repeated group is compared by kind rather than merged, and the two
        // structural forms are the same kind. An object written inline and
        // a block written out must therefore agree, which is what stops a
        // difference in form from reading as a cross-source conflict.
        let base = level(
            A,
            sp(A, 0, 0),
            vec![
                object("service", vec![scalar("port", Scalar::Int(1))]),
                object("service", vec![scalar("port", Scalar::Int(2))]),
            ],
        );
        let over = level(
            A,
            sp(A, 0, 0),
            vec![
                block("service", vec![scalar("port", Scalar::Int(3))]),
                block("service", vec![scalar("port", Scalar::Int(4))]),
            ],
        );
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        assert!(
            !report.has_issues(),
            "an object group and a block group are both structural: {:?}",
            report.issues()
        );
        assert_eq!(merged.iter().count(), 2);
    }

    #[test]
    fn merge_drops_a_commented_entry_nested_in_an_appended_group() {
        // Arrange
        // A repeated-block group appends without recursing, so this covers the
        // inner levels the append never merges.
        let base = level(
            A,
            sp(A, 0, 0),
            vec![
                entry_block(
                    "service",
                    vec![
                        scalar("port", Scalar::Int(1)).into(),
                        scalar("rate", Scalar::Int(0)).as_commented(),
                    ],
                ),
                block("service", vec![scalar("port", Scalar::Int(2))]),
            ],
        );
        let over = level(A, sp(A, 0, 0), vec![]);
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        fn assert_no_commented_entry(fields: &Fields) {
            for entry in fields.entries() {
                assert!(
                    !entry.is_commented(),
                    "commented entry survived: {}",
                    entry.field().name
                );
                if let FieldKind::Block(inner) = &entry.field().kind {
                    assert_no_commented_entry(inner);
                }
            }
        }
        assert_no_commented_entry(&merged);
        let FieldKind::Block(first) = &merged.iter().next().unwrap().kind else {
            panic!("service should stay a block");
        };
        assert!(first.iter().all(|field| field.name != "rate"));
        assert!(!report.has_issues(), "issues: {:?}", report.issues());
    }

    #[test]
    fn merge_drops_a_commented_field_without_a_conflict() {
        // Arrange
        // A commented field reads as absent, so a template placeholder layered
        // by mistake neither conflicts with an active value nor survives into
        // the assembled output.
        let base = entry_level(
            A,
            sp(A, 0, 0),
            vec![
                scalar("port", Scalar::Int(1)).into(),
                scalar("pid_file", Scalar::Int(9)).as_commented(),
            ],
        );
        let over = entry_level(
            A,
            sp(A, 0, 0),
            vec![scalar("port", Scalar::Int(2)).as_commented()],
        );
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        assert_eq!(scalar_of(merged.get("port").unwrap()), &Scalar::Int(1));
        assert!(merged.entries().all(|entry| !entry.is_commented()));
        assert!(merged.iter().all(|field| field.name != "pid_file"));
        assert!(!report.has_issues(), "issues: {:?}", report.issues());
    }

    #[test]
    fn merge_overrides_a_scalar_with_the_higher_precedence_source() {
        // Arrange
        let base = level(A, sp(A, 0, 0), vec![scalar("port", Scalar::Int(1))]);
        let over = level(A, sp(A, 0, 0), vec![scalar("port", Scalar::Int(2))]);
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

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
        let merged = combine(base, over, Verb::Join, &mut report);

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
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        let FieldKind::Block(server) = &merged.get("server").unwrap().kind else {
            panic!("expected a block");
        };
        assert!(server.get("port").is_some());
        assert!(server.get("host").is_some());
        assert!(!report.has_issues());
    }

    #[test]
    fn merge_unifies_block_and_object_forms_keeping_the_base_form() {
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
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        let FieldKind::Block(server) = &merged.get("server").unwrap().kind else {
            panic!("expected the base block form to survive");
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
        let merged = combine(base, over, Verb::Merge, &mut report);

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
        let merged = combine(base, over, Verb::Merge, &mut report);

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
    fn merge_keeps_the_base_doc_on_an_overridden_value() {
        // Arrange
        // `rewrap` keeps the base's doc on a merged block, so the value-level
        // override follows the same rule. The value moves and the doc stays.
        let mut base_field = scalar("port", Scalar::Int(1));
        base_field.doc = Some("The listen port.".to_string());
        let mut over_field = scalar("port", Scalar::Int(2));
        over_field.doc = Some("Override doc.".to_string());
        let base = level(A, sp(A, 0, 0), vec![base_field]);
        let over = level(A, sp(A, 0, 0), vec![over_field]);
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        let field = merged.get("port").unwrap();
        assert_eq!(scalar_of(field), &Scalar::Int(2));
        assert_eq!(field.doc.as_deref(), Some("The listen port."));
    }

    #[test]
    fn repeated_blocks_against_a_scalar_report_a_conflict() {
        // Arrange
        // A scalar override against a repeated block group must report a
        // conflict rather than replace the group, which pins the kind check
        // in the group arm.
        let base = level(
            A,
            sp(A, 0, 0),
            vec![block("server", vec![]), block("server", vec![])],
        );
        let over = level(A, sp(A, 0, 0), vec![scalar("server", Scalar::Int(1))]);
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        assert!(report.has_errors());
        assert_eq!(
            report.issues()[0].message,
            "`server` is a value in one source and a block in another"
        );
        assert!(matches!(
            merged.get("server").unwrap().kind,
            FieldKind::Value(_)
        ));
        assert_eq!(merged.iter().count(), 1);
    }

    #[test]
    fn repeated_blocks_against_repeated_blocks_replace_without_a_conflict() {
        // Arrange
        let base = level(
            A,
            sp(A, 0, 0),
            vec![block("server", vec![]), block("server", vec![])],
        );
        let over = level(
            A,
            sp(A, 0, 0),
            vec![
                block("server", vec![scalar("port", Scalar::Int(1))]),
                block("server", vec![scalar("port", Scalar::Int(2))]),
                block("server", vec![scalar("port", Scalar::Int(3))]),
            ],
        );
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        assert_eq!(merged.iter().count(), 3);
        assert!(!report.has_issues());
    }

    #[test]
    fn merged_level_keeps_the_base_source_and_enclosing() {
        // Arrange
        let base = level(A, sp(A, 3, 7), vec![scalar("port", Scalar::Int(1))]);
        let over = level(B, sp(B, 9, 9), vec![scalar("host", Scalar::Int(2))]);
        let mut report = Report::new();

        // Act
        let merged = combine(base, over, Verb::Merge, &mut report);

        // Assert
        assert_eq!(merged.source(), A);
        assert_eq!(merged.enclosing(), sp(A, 3, 7));
    }
}
