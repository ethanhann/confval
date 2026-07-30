//! KDL write path: serializes a neutral [`Fields`] tree to canonical KDL.
//!
//! This is the inverse of [`parse_kdl_fields`](super::parse_kdl_fields). It
//! builds a kdl-rs document by structure, sets each node's decor by hand, and
//! renders the doc comments an annotated template carries.

use crate::format::EmitError;
use crate::format::emit::{child_path, comment_lines};
use crate::format::field::{Field, FieldKind, Fields, Scalar, Value, ValueKind};
use kdl::{
    KdlDocument, KdlDocumentFormat, KdlEntry, KdlEntryFormat, KdlNode, KdlNodeFormat, KdlValue,
};

/// Serializes a [`Fields`] tree to canonical KDL text.
///
/// This is the inverse of [`parse_kdl_fields`](super::parse_kdl_fields). It
/// builds a kdl-rs document by structure and returns its text, dropping the
/// comments and layout the neutral model never held. A scalar spells as one
/// argument, a sequence of scalars as repeated arguments, an empty sequence as
/// a bare node, and a nested structure as a children block, with values before
/// blocks and a blank line above every block node that follows another
/// structure. Strings keep the quoted spelling. Repeated same-named value
/// fields group into one node, so a parsed repeated-node list round-trips.
///
/// It fails on a [`ValueKind::Other`] and on a sequence KDL cannot spell: one
/// holding a nested sequence, one mixing maps with scalars, and a non-scalar
/// inside a grouped repetition. Every name is representable, because KDL
/// quotes any string as a node name, so neither
/// [`EmitError::UnrepresentableName`] nor [`EmitError::ConflictingName`] can
/// arise. Emit of a populated spec never fails, because populate produces
/// none of the failing shapes.
pub fn emit_kdl(fields: &Fields) -> Result<String, EmitError> {
    let mut document = emit_document(fields, 0, "")?;
    document.set_format(KdlDocumentFormat::default());
    Ok(document.to_string())
}

/// Builds a document for one level, indented for the given nesting depth.
///
/// Value fields emit before blocks, each group in declaration order, and
/// same-named value fields group into one node at the first occurrence's
/// position. Any node with children gets a blank line above it when it follows
/// another structure, and above its doc comment if it has one.
fn emit_document(fields: &Fields, level: usize, path: &str) -> Result<KdlDocument, EmitError> {
    let indent = "  ".repeat(level);
    let mut nodes: Vec<KdlNode> = Vec::new();
    let mut grouped: Vec<&str> = Vec::new();
    for field in fields.iter() {
        let FieldKind::Value(_) = &field.kind else {
            continue;
        };
        if grouped.iter().any(|name| *name == field.name) {
            continue;
        }
        grouped.push(&field.name);
        let group: Vec<&Field> = fields
            .iter()
            .filter(|other| other.name == field.name && matches!(other.kind, FieldKind::Value(_)))
            .collect();
        let child = child_path(path, &field.name);
        // Only one comment can render above the grouped node, so the group
        // takes the first doc any member carries.
        let doc = group.iter().find_map(|member| member.doc.as_deref());
        if group.len() == 1 {
            emit_value_field(&mut nodes, field, doc, level, &indent, &child)?;
        } else {
            let mut node = node_with(&field.name, leading(doc, &indent, !nodes.is_empty()));
            for member in &group {
                let FieldKind::Value(value) = &member.kind else {
                    continue;
                };
                push_grouped_arguments(&mut node, value, &child)?;
            }
            nodes.push(node);
        }
    }
    for field in fields.iter() {
        let FieldKind::Block(inner) = &field.kind else {
            continue;
        };
        let child = child_path(path, &field.name);
        let mut node = node_with(
            &field.name,
            block_leading(field.doc.as_deref(), &indent, !nodes.is_empty()),
        );
        attach_children(&mut node, emit_document(inner, level + 1, &child)?, &indent);
        nodes.push(node);
    }
    let mut document = KdlDocument::new();
    *document.nodes_mut() = nodes;
    Ok(document)
}

/// Emits one unrepeated value field: a scalar or sequence node, a children
/// block for a map, or one node per element for a sequence of maps.
fn emit_value_field(
    nodes: &mut Vec<KdlNode>,
    field: &Field,
    doc: Option<&str>,
    level: usize,
    indent: &str,
    path: &str,
) -> Result<(), EmitError> {
    let FieldKind::Value(value) = &field.kind else {
        return Ok(());
    };
    match &value.kind {
        ValueKind::Scalar(scalar) => {
            let mut node = node_with(&field.name, leading(doc, indent, !nodes.is_empty()));
            node.entries_mut().push(scalar_entry(scalar));
            nodes.push(node);
        }
        ValueKind::Seq(elements) => match classify_sequence(elements, path)? {
            Sequence::Scalars(scalars) => {
                let mut node = node_with(&field.name, leading(doc, indent, !nodes.is_empty()));
                for scalar in scalars {
                    node.entries_mut().push(scalar_entry(scalar));
                }
                nodes.push(node);
            }
            Sequence::Maps(maps) => {
                for (index, inner) in maps.into_iter().enumerate() {
                    let doc = if index == 0 { doc } else { None };
                    let mut node =
                        node_with(&field.name, block_leading(doc, indent, !nodes.is_empty()));
                    attach_children(&mut node, emit_document(inner, level + 1, path)?, indent);
                    nodes.push(node);
                }
            }
        },
        ValueKind::Map(inner) => {
            let mut node = node_with(&field.name, block_leading(doc, indent, !nodes.is_empty()));
            attach_children(&mut node, emit_document(inner, level + 1, path)?, indent);
            nodes.push(node);
        }
        ValueKind::Other(label) => {
            return Err(EmitError::UnrepresentableValue {
                label,
                path: path.to_string(),
            });
        }
    }
    Ok(())
}

/// A sequence's one KDL spelling: repeated arguments when every element is a
/// scalar, repeated nodes when every element is a map.
enum Sequence<'a> {
    Scalars(Vec<&'a Scalar>),
    Maps(Vec<&'a Fields>),
}

/// Classifies a sequence into its spelling, or the label of the element KDL
/// cannot spell. An argument must be a scalar and KDL has no inline array, so
/// a nested sequence and a map mixed with scalars have no representation.
fn classify_sequence<'a>(elements: &'a [Value], path: &str) -> Result<Sequence<'a>, EmitError> {
    let mut scalars = Vec::new();
    let mut maps = Vec::new();
    for element in elements {
        match &element.kind {
            ValueKind::Scalar(scalar) => scalars.push(scalar),
            ValueKind::Map(fields) => maps.push(fields),
            ValueKind::Seq(_) => {
                return Err(EmitError::UnrepresentableValue {
                    label: "nested sequence",
                    path: path.to_string(),
                });
            }
            ValueKind::Other(label) => {
                return Err(EmitError::UnrepresentableValue {
                    label,
                    path: path.to_string(),
                });
            }
        }
    }
    match (scalars.is_empty(), maps.is_empty()) {
        (_, true) => Ok(Sequence::Scalars(scalars)),
        (true, false) => Ok(Sequence::Maps(maps)),
        (false, false) => Err(EmitError::UnrepresentableValue {
            label: "mixed sequence",
            path: path.to_string(),
        }),
    }
}

/// Appends one grouped member's arguments to the shared node. A grouped
/// repetition spells only scalars, because its one node carries arguments and
/// an argument must be a scalar.
fn push_grouped_arguments(node: &mut KdlNode, value: &Value, path: &str) -> Result<(), EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => {
            node.entries_mut().push(scalar_entry(scalar));
            Ok(())
        }
        ValueKind::Seq(elements) => match classify_sequence(elements, path)? {
            Sequence::Scalars(scalars) => {
                for scalar in scalars {
                    node.entries_mut().push(scalar_entry(scalar));
                }
                Ok(())
            }
            Sequence::Maps(_) => Err(EmitError::UnrepresentableValue {
                label: "repeated nested value",
                path: path.to_string(),
            }),
        },
        ValueKind::Map(_) => Err(EmitError::UnrepresentableValue {
            label: "repeated nested value",
            path: path.to_string(),
        }),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// A node with its leading decor set and a newline terminator. The name
/// renders bare when it is a plain identifier and quoted otherwise, which
/// kdl-rs decides, so every name is representable.
fn node_with(name: &str, leading: String) -> KdlNode {
    let mut node = KdlNode::new(name);
    node.set_format(KdlNodeFormat {
        leading,
        terminator: "\n".to_string(),
        ..KdlNodeFormat::default()
    });
    node
}

/// The leading decor for a value node: its doc comment as `// line` comments,
/// each at the node's indentation, followed by the indent itself.
fn leading(doc: Option<&str>, indent: &str, _follows: bool) -> String {
    let mut out = String::new();
    if let Some(text) = doc {
        for line in comment_lines(text) {
            out.push_str(indent);
            if line.is_empty() {
                out.push_str("//\n");
            } else {
                out.push_str("// ");
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
    out.push_str(indent);
    out
}

/// The leading decor for a block node: a blank line when the node follows
/// another structure at its level, then the doc comment and indent.
fn block_leading(doc: Option<&str>, indent: &str, follows: bool) -> String {
    let mut out = String::new();
    if follows {
        out.push('\n');
    }
    out.push_str(&leading(doc, indent, follows));
    out
}

/// Attaches a children block, with the newline after the opening brace and the
/// parent's indent before the closing brace, so the brace lines up with the
/// node that opened it.
fn attach_children(node: &mut KdlNode, mut children: KdlDocument, indent: &str) {
    children.set_format(KdlDocumentFormat {
        leading: "\n".to_string(),
        trailing: indent.to_string(),
    });
    if let Some(format) = node.format_mut() {
        format.before_children = " ".to_string();
    }
    *node.children_mut() = Some(children);
}

/// One argument entry, with the canonical text set explicitly, because
/// kdl-rs's own rendering spells an identifier-shaped string bare and the
/// canonical form keeps every string quoted.
fn scalar_entry(scalar: &Scalar) -> KdlEntry {
    let (value, repr) = match scalar {
        Scalar::String(string) => (KdlValue::String(string.clone()), quoted(string)),
        Scalar::Int(int) => (KdlValue::Integer(i128::from(*int)), int.to_string()),
        Scalar::Float(float) => (KdlValue::Float(*float), float_repr(*float)),
        Scalar::Bool(boolean) => (KdlValue::Bool(*boolean), format!("#{boolean}")),
        Scalar::Unparsed(raw) => (KdlValue::String(raw.clone()), quoted(raw)),
    };
    let mut entry = KdlEntry::new(value);
    entry.set_format(KdlEntryFormat {
        value_repr: repr,
        leading: " ".to_string(),
        ..KdlEntryFormat::default()
    });
    entry
}

/// The quoted spelling of a string, with the escapes KDL 2.0 defines and a
/// unicode escape for any other control character, so an adversarial string
/// still reparses.
fn quoted(string: &str) -> String {
    let mut out = String::with_capacity(string.len() + 2);
    out.push('"');
    for character in string.chars() {
        match character {
            '\\' | '"' => {
                out.push('\\');
                out.push(character);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{{{:x}}}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// A float's literal: the shortest round-trip text for a finite value, which
/// always carries a decimal point or an exponent, and the KDL 2.0 keyword for
/// a non-finite one.
fn float_repr(float: f64) -> String {
    if float == f64::INFINITY {
        "#inf".to_string()
    } else if float == f64::NEG_INFINITY {
        "#-inf".to_string()
    } else if float.is_nan() {
        "#nan".to_string()
    } else {
        format!("{float:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_kdl_fields;
    use super::*;
    use crate::diagnostic::Report;
    use crate::format::field::FromFields;
    use crate::format::parse::{
        parse_float_field, parse_int_field, parse_string_field, parse_string_list_field,
        parse_struct_list_field,
    };
    use crate::source::{Located, SourceMap};

    struct Probe;
    impl FromFields for Probe {
        fn from_fields(_: &Fields, _: &mut Report) -> Option<Self> {
            Some(Probe)
        }
    }

    fn scalar(name: &str, scalar: Scalar) -> Field {
        Field::detached_value(name, Value::detached(ValueKind::Scalar(scalar)))
    }

    fn seq(name: &str, elements: Vec<ValueKind>) -> Field {
        let values = elements.into_iter().map(Value::detached).collect();
        Field::detached_value(name, Value::detached(ValueKind::Seq(values)))
    }

    fn reparse(text: &str) -> Fields {
        let mut sources = SourceMap::new();
        let id = sources.add("emitted.kdl", text.to_string());
        let mut report = Report::new();
        let fields = parse_kdl_fields(&sources, id, &mut report).unwrap();
        assert!(
            !report.has_issues(),
            "reparse issues: {:?}",
            report.issues()
        );
        fields
    }

    #[test]
    fn emit_kdl_writes_canonical_text() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("hostname", Scalar::String("api".to_string())),
            scalar("port", Scalar::Int(8080)),
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            ),
        ]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        // Strings keep the quoted spelling, the block body is indented one
        // level, the closing brace lines up with the opener, and a blank line
        // separates the block from the value above it.
        assert_eq!(
            text,
            "hostname \"api\"\nport 8080\n\nlimits {\n  max_body_mb 16\n}\n"
        );
    }

    #[test]
    fn emit_kdl_orders_values_before_blocks() {
        // Arrange
        let fields = Fields::detached(vec![
            Field::detached_block(
                "sprocket",
                Fields::detached(vec![scalar("max_height", Scalar::Int(32))]),
            ),
            scalar("max_weight", Scalar::Int(16)),
        ]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "max_weight 16\n\nsprocket {\n  max_height 32\n}\n");
    }

    #[test]
    fn emit_kdl_writes_a_sequence_as_repeated_arguments() {
        // Arrange
        let fields = Fields::detached(vec![seq(
            "allow",
            vec![
                ValueKind::Scalar(Scalar::String("a".to_string())),
                ValueKind::Scalar(Scalar::String("b".to_string())),
            ],
        )]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "allow \"a\" \"b\"\n");
        let round = reparse(&text);
        let mut report = Report::new();
        let list = parse_string_list_field(round.get("allow").unwrap(), &mut report).unwrap();
        assert_eq!(list.value.len(), 2);
    }

    #[test]
    fn emit_kdl_writes_an_empty_sequence_as_a_bare_node() {
        // Arrange
        let fields = Fields::detached(vec![seq("allow", vec![])]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "allow\n");
        let round = reparse(&text);
        let mut report = Report::new();
        let list = parse_string_list_field(round.get("allow").unwrap(), &mut report).unwrap();
        assert!(list.value.is_empty());
    }

    #[test]
    fn emit_kdl_collapses_a_one_element_sequence_to_a_scalar_spelling() {
        // Arrange
        // The reparse reads one argument as a scalar, so round-trip equality
        // at the spec level rests on the widened list parser.
        let fields = Fields::detached(vec![seq(
            "allow",
            vec![ValueKind::Scalar(Scalar::String("a".to_string()))],
        )]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "allow \"a\"\n");
        let round = reparse(&text);
        let mut report = Report::new();
        let list = parse_string_list_field(round.get("allow").unwrap(), &mut report).unwrap();
        assert_eq!(list.value.len(), 1);
    }

    #[test]
    fn emit_kdl_writes_an_empty_block_that_reparses() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_block(
            "empty",
            Fields::detached(vec![]),
        )]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "empty {\n}\n");
        let round = reparse(&text);
        assert!(matches!(
            round.get("empty").unwrap().kind,
            FieldKind::Block(_)
        ));
    }

    #[test]
    fn emit_kdl_writes_repeated_blocks_as_repeated_nodes() {
        // Arrange
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![block(1), block(2)]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "service {\n  port 1\n}\n\nservice {\n  port 2\n}\n");
        let round = reparse(&text);
        let mut report = Report::new();
        let mut services: Vec<Located<Probe>> = Vec::new();
        for field in round.iter() {
            parse_struct_list_field(&mut services, field, &mut report);
        }
        assert_eq!(services.len(), 2);
    }

    #[test]
    fn emit_kdl_writes_a_sequence_of_maps_as_repeated_nodes() {
        // Arrange
        let map =
            |port: i64| ValueKind::Map(Fields::detached(vec![scalar("port", Scalar::Int(port))]));
        let fields = Fields::detached(vec![seq("service", vec![map(1), map(2)])]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text.matches("service {").count(), 2, "got: {text}");
        let round = reparse(&text);
        let mut report = Report::new();
        let mut services: Vec<Located<Probe>> = Vec::new();
        for field in round.iter() {
            parse_struct_list_field(&mut services, field, &mut report);
        }
        assert_eq!(services.len(), 2);
    }

    #[test]
    fn emit_kdl_groups_repeated_value_fields_into_one_node() {
        // Arrange
        // Only a parsed KDL document produces this shape, so grouping keeps
        // the repeated-node list spelling emittable.
        let fields = Fields::detached(vec![
            scalar("allow", Scalar::String("a".to_string())),
            scalar("name", Scalar::String("x".to_string())),
            scalar("allow", Scalar::String("b".to_string())),
        ]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "allow \"a\" \"b\"\nname \"x\"\n");
    }

    #[test]
    fn emit_kdl_writes_a_map_as_a_children_block() {
        // Arrange
        let map = Fields::detached(vec![scalar("cert", Scalar::String("a.pem".to_string()))]);
        let fields = Fields::detached(vec![Field::detached_value(
            "tls",
            Value::detached(ValueKind::Map(map)),
        )]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "tls {\n  cert \"a.pem\"\n}\n");
    }

    #[test]
    fn emit_kdl_rejects_a_mixed_sequence() {
        // Arrange
        let fields = Fields::detached(vec![seq(
            "items",
            vec![
                ValueKind::Scalar(Scalar::Int(1)),
                ValueKind::Map(Fields::detached(vec![])),
            ],
        )]);

        // Act
        let result = emit_kdl(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "mixed sequence",
                path: "items".to_string(),
            })
        );
    }

    #[test]
    fn emit_kdl_rejects_a_nested_sequence() {
        // Arrange
        let fields = Fields::detached(vec![seq(
            "matrix",
            vec![ValueKind::Seq(vec![Value::detached(ValueKind::Scalar(
                Scalar::Int(1),
            ))])],
        )]);

        // Act
        let result = emit_kdl(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "nested sequence",
                path: "matrix".to_string(),
            })
        );
    }

    #[test]
    fn emit_kdl_rejects_an_unrepresentable_value() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_value(
            "when",
            Value::detached(ValueKind::Other("null")),
        )]);

        // Act
        let result = emit_kdl(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "null",
                path: "when".to_string(),
            })
        );
    }

    #[test]
    fn emit_kdl_round_trips_non_finite_floats() {
        // KDL 2.0 spells infinity and NaN as keywords, so these emit rather
        // than fail, matching TOML where HCL refuses.
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            // Arrange
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(value))]);

            // Act
            let text = emit_kdl(&fields).unwrap();

            // Assert
            let round = reparse(&text);
            let mut report = Report::new();
            let parsed = parse_float_field(round.get("rate").unwrap(), &mut report).unwrap();
            if value.is_nan() {
                assert!(parsed.value.is_nan(), "emitted: {text:?}");
            } else {
                assert_eq!(parsed.value, value, "emitted: {text:?}");
            }
            assert!(!report.has_issues());
        }
    }

    #[test]
    fn emit_kdl_keeps_the_float_spelling() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("whole", Scalar::Float(4.0)),
            scalar("count", Scalar::Int(4)),
        ]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "whole 4.0\ncount 4\n");
        let round = reparse(&text);
        let mut report = Report::new();
        assert!(parse_int_field(round.get("whole").unwrap(), &mut report).is_none());
        report = Report::new();
        assert_eq!(
            parse_int_field(round.get("count").unwrap(), &mut report)
                .unwrap()
                .value,
            4
        );
    }

    #[test]
    fn emit_kdl_round_trips_an_adversarial_string() {
        // Arrange
        // Escaping is this module's own, so this guards the quoted spelling
        // against quotes, backslashes, line breaks, tabs, unicode, and control
        // characters.
        let hostile = "quote\" backslash\\ newline\n tab\t snowman\u{2603} del\u{7f} bel\u{7}";
        let fields = Fields::detached(vec![scalar(
            "greeting",
            Scalar::String(hostile.to_string()),
        )]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        let round = reparse(&text);
        let mut report = Report::new();
        let parsed = parse_string_field(round.get("greeting").unwrap(), &mut report).unwrap();
        assert_eq!(parsed.value, hostile, "emitted: {text:?}");
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_kdl_quotes_an_identifier_shaped_string() {
        // Arrange
        // kdl-rs's own rendering would spell this bare, and the canonical form
        // keeps every string quoted.
        let fields = Fields::detached(vec![scalar("mode", Scalar::String("enforce".to_string()))]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "mode \"enforce\"\n");
    }

    #[test]
    fn emit_kdl_quotes_a_non_identifier_node_name() {
        // Arrange
        let fields = Fields::detached(vec![scalar("weird key", Scalar::Int(1))]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert!(text.contains("\"weird key\""), "got: {text}");
        let round = reparse(&text);
        assert!(round.get("weird key").is_some());
    }

    #[test]
    fn emit_kdl_renders_doc_comments_above_their_nodes() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("port", Scalar::Int(1)).with_doc(Some("The port.".to_string())),
            Field::detached_block(
                "limits",
                Fields::detached(vec![
                    scalar("max_body_mb", Scalar::Int(16))
                        .with_doc(Some("Max body size.".to_string())),
                ]),
            )
            .with_doc(Some("Request limits.".to_string())),
        ]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        // The blank line goes above the block's comment, and the nested
        // comment carries its field's indentation.
        assert_eq!(
            text,
            "// The port.\nport 1\n\n// Request limits.\nlimits {\n  // Max body size.\n  max_body_mb 16\n}\n"
        );
        reparse(&text);
    }

    #[test]
    fn emit_kdl_writes_a_value_beside_a_same_named_block() {
        // Arrange
        // HCL spells `x = 1` next to `x { }`, so a parsed Fields can hold
        // both, and KDL spells the pair as two nodes.
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![scalar("y", Scalar::Int(2))])),
        ]);

        // Act
        let text = emit_kdl(&fields).unwrap();

        // Assert
        assert_eq!(text, "x 1\n\nx {\n  y 2\n}\n");
        let round = reparse(&text);
        assert_eq!(round.iter().count(), 2);
    }

    #[test]
    fn emit_kdl_rejects_a_map_inside_a_grouped_repetition() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_value(
                "x",
                Value::detached(ValueKind::Map(Fields::detached(vec![]))),
            ),
        ]);

        // Act
        let result = emit_kdl(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "repeated nested value",
                path: "x".to_string(),
            })
        );
    }
}
