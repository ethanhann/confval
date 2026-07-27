//! Builds a nested [`Fields`] tree from the flat, decoded assignments of a
//! non-file source.
//!
//! Environment variables and command line flags each decode to a path of
//! segments and a raw value. This module assembles those into the same neutral
//! `Fields` a file frontend produces. Every value becomes an unparsed literal,
//! so a leaf parser coerces it to the field's declared type.

use crate::diagnostic::Report;
use crate::format::{Field, FieldKind, Fields, Scalar, Value, ValueKind};
use crate::source::{SourceId, Span};
use std::collections::BTreeMap;

/// One decoded assignment, a path of segments with its raw value. The value is
/// already registered as its own synthetic source.
pub(crate) struct Leaf {
    pub path: Vec<String>,
    pub raw: String,
    pub source: SourceId,
    pub span: Span,
}

enum Node {
    Leaf(Leaf),
    Branch(BTreeMap<String, Node>),
}

/// Assembles the leaves into a `Fields` rooted at `root`.
///
/// Container levels carry the `root` source, so a missing-field error inside a
/// synthesized block points at the provider rather than at a value. Each leaf
/// carries its own value source.
pub(crate) fn build(root: SourceId, leaves: Vec<Leaf>, report: &mut Report) -> Fields {
    let mut tree: BTreeMap<String, Node> = BTreeMap::new();
    for leaf in leaves {
        let path = leaf.path.clone();
        insert(&mut tree, &path, leaf, report);
    }
    to_fields(root, tree)
}

fn insert(branch: &mut BTreeMap<String, Node>, path: &[String], leaf: Leaf, report: &mut Report) {
    let Some((head, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        match branch.get(head) {
            Some(Node::Branch(_)) => report_collision(head, &leaf, report),
            // A repeated flag keeps the last assignment.
            _ => {
                branch.insert(head.clone(), Node::Leaf(leaf));
            }
        }
    } else {
        let entry = branch
            .entry(head.clone())
            .or_insert_with(|| Node::Branch(BTreeMap::new()));
        match entry {
            Node::Branch(sub) => insert(sub, rest, leaf, report),
            Node::Leaf(_) => report_collision(head, &leaf, report),
        }
    }
}

fn report_collision(name: &str, leaf: &Leaf, report: &mut Report) {
    report
        .error(format!(
            "`{name}` is set both as a value and as a nested group"
        ))
        .at(leaf.span)
        .emit();
}

fn to_fields(source: SourceId, tree: BTreeMap<String, Node>) -> Fields {
    let items = tree
        .into_iter()
        .map(|(name, node)| node_to_field(source, name, node))
        .collect();
    Fields::new(source, Span::new(source, 0, 0), items)
}

fn node_to_field(source: SourceId, name: String, node: Node) -> Field {
    match node {
        Node::Leaf(leaf) => Field {
            name,
            name_span: leaf.span,
            span: leaf.span,
            source: leaf.source,
            kind: FieldKind::Value(Value {
                span: leaf.span,
                kind: ValueKind::Scalar(Scalar::Unparsed(leaf.raw)),
            }),
            doc: None,
        },
        Node::Branch(sub) => Field {
            name,
            name_span: Span::new(source, 0, 0),
            span: Span::new(source, 0, 0),
            source,
            kind: FieldKind::Block(to_fields(source, sub)),
            doc: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: SourceId = SourceId(0);
    const VALUE: SourceId = SourceId(1);

    fn leaf(path: &[&str], raw: &str) -> Leaf {
        Leaf {
            path: path.iter().map(|segment| segment.to_string()).collect(),
            raw: raw.to_string(),
            source: VALUE,
            span: Span::new(VALUE, 0, raw.len() as u32),
        }
    }

    fn unparsed(field: &Field) -> &str {
        let FieldKind::Value(Value {
            kind: ValueKind::Scalar(Scalar::Unparsed(raw)),
            ..
        }) = &field.kind
        else {
            panic!("expected an unparsed scalar");
        };
        raw
    }

    #[test]
    fn a_single_segment_becomes_a_top_level_field() {
        // Arrange
        let mut report = Report::new();
        // Act
        let fields = build(ROOT, vec![leaf(&["port"], "8080")], &mut report);
        // Assert
        assert_eq!(unparsed(fields.get("port").unwrap()), "8080");
        assert!(!report.has_issues());
    }

    #[test]
    fn deeper_segments_nest_under_a_shared_parent() {
        // Arrange
        let leaves = vec![
            leaf(&["server", "host"], "h"),
            leaf(&["server", "port"], "1"),
        ];
        let mut report = Report::new();
        // Act
        let fields = build(ROOT, leaves, &mut report);
        // Assert
        let FieldKind::Block(server) = &fields.get("server").unwrap().kind else {
            panic!("expected a nested block");
        };
        assert_eq!(unparsed(server.get("host").unwrap()), "h");
        assert_eq!(unparsed(server.get("port").unwrap()), "1");
    }

    #[test]
    fn a_value_and_a_group_at_the_same_key_is_a_collision() {
        // Arrange
        let leaves = vec![leaf(&["server"], "flat"), leaf(&["server", "port"], "1")];
        let mut report = Report::new();
        // Act
        let _ = build(ROOT, leaves, &mut report);
        // Assert
        assert!(report.has_errors());
        assert!(
            report.issues()[0]
                .message
                .contains("set both as a value and as a nested group")
        );
    }
}
