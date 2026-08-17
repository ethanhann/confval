//! YAML write path: serializes a neutral [`Fields`] tree to canonical YAML.
//!
//! This is the inverse of [`parse_yaml_fields`](super::parse_yaml_fields). It
//! writes the text directly, because YAML's layout is small enough to state in
//! one place: block style throughout, two-space indentation, one entry per
//! line, a blank line above every member that renders as a block, and a
//! trailing newline. Three things render flow: a sequence whose elements are
//! all scalars, an empty sequence, and an empty mapping.
//!
//! The sibling `text` module holds the mechanics this layout rests on: how a
//! scalar, a key, and a string are written, and how a rendered block gains a
//! sequence marker or a comment marker.

use super::member::{Member, Rendered, Shape, members_of, shape_of, shape_of_value};
use super::text::{comment_out, splice_dash, write_key, write_scalar};
use crate::format::EmitError;
use crate::format::emit::indent;
use crate::format::emit::{
    child_path, comment_lines, grouped_elements, refuse_label, value_beside_block,
};
use crate::format::field::{Fields, Value, ValueKind};

/// Serializes a [`Fields`] tree to canonical YAML text.
///
/// This is the inverse of [`parse_yaml_fields`](super::parse_yaml_fields). It
/// returns block-style YAML, dropping the layout the neutral model never held.
/// Values emit before blocks at each level, each group in declaration order.
///
/// Strings always emit double-quoted, so a string the core schema would resolve
/// to something else, `no` or `123` or `null`, reads back as the string it was.
/// A key emits bare when it is an ASCII identifier and double-quoted otherwise,
/// and a key is a name rather than a typed value, so both read back as the same
/// field name. [`EmitError::UnrepresentableName`] arises only for a name whose
/// written form runs past YAML's 1024-character simple-key limit, which the
/// parser would refuse to read back.
///
/// Doc comments render as `# ` lines above their entry, and a commented entry
/// renders behind a spaceless `#` with its indentation outside the marker, so
/// uncommenting is deleting one character.
///
/// Same-named fields at one level group into one member holding a sequence, so
/// a tree parsed from duplicate keys still emits. Grouping collapses several
/// fields into one member, so the round trip over duplicates holds at the
/// walk's resolution rather than at the `Fields` level.
///
/// It fails on a [`ValueKind::Other`] and on a value beside a same-named block,
/// whose only YAML form is a duplicate key. Emit of a populated spec never
/// fails, because populate produces identifier names, ordinary scalars with
/// every float writable, no `Other`, and no repetition.
pub fn emit_yaml(fields: &Fields) -> Result<String, EmitError> {
    let mut out = String::new();
    write_level(&mut out, fields, 0, "")?;
    if out.is_empty() {
        out.push_str("{}\n");
    }
    Ok(out)
}

/// Writes one mapping level, each entry at `level`.
fn write_level(
    out: &mut String,
    fields: &Fields,
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    if let Some(error) = refuse_label(fields, path) {
        return Err(error);
    }
    if let Some(name) = value_beside_block(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    for (index, rendered) in members_of(fields).iter().enumerate() {
        write_entry(out, rendered, level, path, index > 0)?;
    }
    Ok(())
}

/// Writes one member: its blank line, its doc comment, and its entry.
///
/// `follows` is whether something precedes it at this level, which decides the
/// blank line above a block-rendered member.
fn write_entry(
    out: &mut String,
    rendered: &Rendered,
    level: usize,
    path: &str,
    follows: bool,
) -> Result<(), EmitError> {
    let mut key = String::new();
    write_key(&mut key, rendered.name);
    // YAML caps a simple key at 1024 characters, so a longer written form
    // would emit text the parser refuses to read back.
    if key.chars().count() > 1024 {
        return Err(EmitError::UnrepresentableName {
            name: rendered.name.to_string(),
            path: path.to_string(),
        });
    }
    let shape = shape_of(&rendered.member);
    if follows && shape == Shape::Block {
        out.push('\n');
    }
    write_doc(out, rendered.doc, level);
    let path = child_path(path, rendered.name);
    let mut body = String::new();
    indent(&mut body, level);
    body.push_str(&key);
    body.push(':');
    match shape {
        Shape::Inline => {
            body.push(' ');
            write_inline(&mut body, &rendered.member, &path)?;
            body.push('\n');
        }
        Shape::Block => {
            body.push('\n');
            write_block(&mut body, &rendered.member, level, &path)?;
        }
    }
    if rendered.commented {
        out.push_str(&comment_out(&body));
    } else {
        out.push_str(&body);
    }
    Ok(())
}

/// Writes a doc comment as `# ` lines at the entry's indentation.
fn write_doc(out: &mut String, doc: Option<&str>, level: usize) {
    let Some(doc) = doc else {
        return;
    };
    for line in comment_lines(doc) {
        indent(out, level);
        if line.is_empty() {
            out.push_str("#\n");
        } else {
            out.push_str("# ");
            out.push_str(&line);
            out.push('\n');
        }
    }
}

/// Writes a member that fits after `key: `.
fn write_inline(out: &mut String, member: &Member, path: &str) -> Result<(), EmitError> {
    match member {
        Member::Values(group) => match group.as_slice() {
            [only] => write_value_inline(out, only, path),
            _ => write_flow(out, &grouped_elements(group), path),
        },
        // `shape_of` sends only an empty block here. Refusing a non-empty one
        // means a change to that predicate fails loudly rather than writing an
        // empty mapping over real entries.
        Member::Blocks(group) => empty_or_refuse(out, group.first().copied(), path),
    }
}

/// Writes a collection with no active field as `{}`, and refuses one that
/// holds an active field.
///
/// Every caller reaches this only for a level the shape classification called
/// inline, which it does only when the level has no active field. Its
/// commented entries are dropped, the same way a commented block renders.
/// Refusing rather than writing `{}` means a classifier that stops agreeing
/// with its writer reports instead of silently emitting an empty mapping over
/// real entries.
fn empty_or_refuse(out: &mut String, inner: Option<&Fields>, path: &str) -> Result<(), EmitError> {
    if inner.is_some_and(|fields| fields.iter().next().is_none()) {
        out.push_str("{}");
        return Ok(());
    }
    Err(EmitError::UnrepresentableValue {
        label: "misclassified mapping",
        path: path.to_string(),
    })
}

/// Writes a member whose body renders on the lines below `key:`.
fn write_block(
    out: &mut String,
    member: &Member,
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    match member {
        Member::Values(group) => match group.as_slice() {
            [only] => write_value_block(out, only, level + 1, path),
            _ => write_sequence(out, &grouped_elements(group), level + 1, path),
        },
        Member::Blocks(group) => match group.as_slice() {
            [only] => write_level(out, only, level + 1, path),
            _ => {
                for inner in group.iter() {
                    let mut body = String::new();
                    write_level(&mut body, inner, level + 2, path)?;
                    splice_dash(out, &body, level + 1);
                }
                Ok(())
            }
        },
    }
}

/// Writes one value on the key's line.
fn write_value_inline(out: &mut String, value: &Value, path: &str) -> Result<(), EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => {
            write_scalar(out, scalar);
            Ok(())
        }
        ValueKind::Seq(elements) => write_flow(out, &elements.iter().collect::<Vec<_>>(), path),
        ValueKind::Map(inner) => empty_or_refuse(out, Some(inner), path),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// Writes one value's body on the lines below its key.
fn write_value_block(
    out: &mut String,
    value: &Value,
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    match &value.kind {
        ValueKind::Map(inner) => write_level(out, inner, level, path),
        ValueKind::Seq(elements) => {
            write_sequence(out, &elements.iter().collect::<Vec<_>>(), level, path)
        }
        // `shape_of_value` sends only a map or a sequence here, so a scalar
        // reaching this arm means the two disagree.
        ValueKind::Scalar(_) => Err(EmitError::UnrepresentableValue {
            label: "misclassified scalar",
            path: path.to_string(),
        }),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// Writes a flow sequence, `[1, 2]`, which every element fits on one line of.
fn write_flow(out: &mut String, elements: &[&Value], path: &str) -> Result<(), EmitError> {
    out.push('[');
    for (index, element) in elements.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        write_value_inline(out, element, path)?;
    }
    out.push(']');
    Ok(())
}

/// Writes a block sequence, one `- ` element per line, with the markers at
/// `level`.
fn write_sequence(
    out: &mut String,
    elements: &[&Value],
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    for element in elements {
        match shape_of_value(element) {
            Shape::Inline => {
                indent(out, level);
                out.push_str("- ");
                write_value_inline(out, element, path)?;
                out.push('\n');
            }
            Shape::Block => {
                let mut body = String::new();
                write_value_block(&mut body, element, level + 1, path)?;
                splice_dash(out, &body, level);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::parse_yaml_fields;
    use super::*;
    use crate::diagnostic::Report;
    use crate::format::field::{Field, FieldKind, Scalar};
    use crate::format::parse::{parse_float_field, parse_string_field, parse_string_list_field};
    use crate::source::SourceMap;

    fn scalar(name: &str, scalar: Scalar) -> Field {
        Field::detached_value(name, Value::detached(ValueKind::Scalar(scalar)))
    }

    fn text(name: &str, value: &str) -> Field {
        scalar(name, Scalar::String(value.to_string()))
    }

    fn seq(name: &str, elements: Vec<ValueKind>) -> Field {
        let values = elements.into_iter().map(Value::detached).collect();
        Field::detached_value(name, Value::detached(ValueKind::Seq(values)))
    }

    fn map(fields: Vec<Field>) -> ValueKind {
        ValueKind::Map(Fields::detached(fields))
    }

    fn reparse(text: &str) -> Fields {
        let mut sources = SourceMap::new();
        let id = sources.add("emitted.yaml", text.to_string());
        let mut report = Report::new();
        let fields = parse_yaml_fields(&sources, id, &mut report)
            .unwrap_or_else(|| panic!("emitted text should parse:\n{text}"));
        assert!(
            !report.has_issues(),
            "reparse issues: {:?}",
            report.issues()
        );
        fields
    }

    #[test]
    fn emit_yaml_writes_canonical_text() {
        // Arrange
        let fields = Fields::detached(vec![
            text("hostname", "api"),
            scalar("port", Scalar::Int(8080)),
            scalar("daemon", Scalar::Bool(false)),
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            ),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        // Two-space indentation, a blank line above the block, and a trailing
        // newline.
        assert_eq!(
            out,
            "hostname: \"api\"\nport: 8080\ndaemon: false\n\nlimits:\n  max_body_mb: 16\n"
        );
        reparse(&out);
    }

    #[test]
    fn emit_yaml_orders_values_before_blocks() {
        // Arrange
        let fields = Fields::detached(vec![
            Field::detached_block(
                "sprocket",
                Fields::detached(vec![scalar("max_height", Scalar::Int(32))]),
            ),
            scalar("max_weight", Scalar::Int(16)),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "max_weight: 16\n\nsprocket:\n  max_height: 32\n");
    }

    #[test]
    fn emit_yaml_is_idempotent_over_its_own_output() {
        // Arrange
        // The blank line keys on how a member renders rather than on its field
        // kind. A kind-keyed rule would space the blocks on the first emit and
        // drop the spacing on the second, because a reparse yields map values.
        let fields = Fields::detached(vec![
            text("hostname", "api"),
            Field::detached_block("tls", Fields::detached(vec![text("cert", "c.pem")])),
            Field::detached_value(
                "limits",
                Value::detached(map(vec![scalar("max", Scalar::Int(1))])),
            ),
        ]);

        // Act
        let once = emit_yaml(&fields).unwrap();
        let twice = emit_yaml(&reparse(&once)).unwrap();

        // Assert
        assert_eq!(once, twice, "first emit:\n{once}");
        // The map value sorts with the values and the block after them, and
        // the second pass keeps both blank lines even though the reparse turned
        // the block into a map value.
        assert_eq!(
            once,
            "hostname: \"api\"\n\nlimits:\n  max: 1\n\ntls:\n  cert: \"c.pem\"\n"
        );
    }

    #[test]
    fn emit_yaml_dedents_an_inline_member_after_a_block_one() {
        // Arrange
        // A map value renders as a block and sorts with the values, so an
        // inline value can follow it at the same level. The reader has only the
        // indentation to close the block on.
        let fields = Fields::detached(vec![
            Field::detached_value(
                "limits",
                Value::detached(map(vec![scalar("max", Scalar::Int(1))])),
            ),
            scalar("port", Scalar::Int(8080)),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "limits:\n  max: 1\nport: 8080\n");
        let round = reparse(&out);
        assert!(
            round.get("port").is_some(),
            "the dedented member must survive"
        );
        assert_eq!(round.iter().count(), 2);
    }

    #[test]
    fn emit_yaml_writes_a_scalar_sequence_in_flow_style() {
        // Arrange
        let fields = Fields::detached(vec![seq(
            "allow",
            vec![
                ValueKind::Scalar(Scalar::String("a".to_string())),
                ValueKind::Scalar(Scalar::String("b".to_string())),
            ],
        )]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "allow: [\"a\", \"b\"]\n");
        let round = reparse(&out);
        let mut report = Report::new();
        assert_eq!(
            parse_string_list_field(round.get("allow").unwrap(), &mut report)
                .unwrap()
                .value
                .len(),
            2
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_yaml_writes_an_empty_sequence_and_an_empty_mapping_inline() {
        // Arrange
        let fields = Fields::detached(vec![
            seq("allow", vec![]),
            Field::detached_value("tls", Value::detached(map(vec![]))),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        // Neither is block-rendered, so neither takes a blank line.
        assert_eq!(out, "allow: []\ntls: {}\n");
        reparse(&out);
    }

    #[test]
    fn emit_yaml_writes_an_empty_document_as_an_empty_mapping() {
        // Arrange
        let fields = Fields::detached(vec![]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "{}\n");
        assert_eq!(reparse(&out).iter().count(), 0);
    }

    #[test]
    fn emit_yaml_writes_a_structural_sequence_in_block_style() {
        // Arrange
        let element = |port: i64| map(vec![scalar("port", Scalar::Int(port))]);
        let fields = Fields::detached(vec![seq("service", vec![element(1), element(2)])]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        // A mapping element opens on its own marker's line.
        assert_eq!(out, "service:\n  - port: 1\n  - port: 2\n");
        reparse(&out);
    }

    #[test]
    fn emit_yaml_keeps_a_multi_entry_element_under_its_marker() {
        // Arrange
        let element = |port: i64| map(vec![scalar("port", Scalar::Int(port)), text("host", "h")]);
        let fields = Fields::detached(vec![seq("service", vec![element(1), element(2)])]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(
            out,
            "service:\n  - port: 1\n    host: \"h\"\n  - port: 2\n    host: \"h\"\n"
        );
        reparse(&out);
    }

    #[test]
    fn emit_yaml_writes_a_nested_sequence_element_in_flow_style() {
        // Arrange
        let row = |first: i64| {
            ValueKind::Seq(vec![Value::detached(ValueKind::Scalar(Scalar::Int(first)))])
        };
        let fields = Fields::detached(vec![seq("matrix", vec![row(1), row(2)])]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "matrix:\n  - [1]\n  - [2]\n");
        reparse(&out);
    }

    #[test]
    fn emit_yaml_groups_repeated_value_fields_into_one_sequence_member() {
        // Arrange
        // Only a parsed document with duplicate keys produces this shape.
        let fields = Fields::detached(vec![
            text("allow", "a"),
            text("name", "x"),
            text("allow", "b"),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "allow: [\"a\", \"b\"]\nname: \"x\"\n");
    }

    #[test]
    fn emit_yaml_flattens_a_sequence_occurrence_into_the_grouped_member() {
        // Arrange
        // A sequence occurrence contributes its elements, and a scalar or a
        // mapping contributes itself as one element.
        let fields = Fields::detached(vec![
            seq(
                "allow",
                vec![
                    ValueKind::Scalar(Scalar::Int(1)),
                    ValueKind::Scalar(Scalar::Int(2)),
                ],
            ),
            scalar("allow", Scalar::Int(3)),
            Field::detached_value("allow", Value::detached(map(vec![]))),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "allow:\n  - 1\n  - 2\n  - 3\n  - {}\n");
    }

    #[test]
    fn emit_yaml_keeps_a_repeated_empty_block_as_an_element() {
        // Arrange
        // An HCL, TOML, or KDL parse of two same-named empty blocks reaches
        // emit as this shape. Writing the group without a marker for the empty
        // one would drop it from the sequence with no diagnostic.
        let block = |inner: Vec<Field>| Field::detached_block("svc", Fields::detached(inner));
        let fields = Fields::detached(vec![
            block(vec![text("name", "a")]),
            block(vec![]),
            block(vec![text("name", "c")]),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "svc:\n  - name: \"a\"\n  - {}\n  - name: \"c\"\n");
        let round = reparse(&out);
        let FieldKind::Value(value) = &round.get("svc").unwrap().kind else {
            panic!("svc should be an attribute value");
        };
        let ValueKind::Seq(elements) = &value.kind else {
            panic!("svc should reparse as a sequence");
        };
        assert_eq!(elements.len(), 3, "every element must survive");
    }

    #[test]
    fn emit_yaml_keeps_an_all_commented_element_as_an_element() {
        // Arrange
        // A template renders an element that sets nothing. Its body is comment
        // text alone, which carries no line the sequence marker can take.
        let block = |inner: Fields| Field::detached_block("svc", inner);
        let fields = Fields::detached(vec![
            block(Fields::detached(vec![scalar("port", Scalar::Int(9))])),
            block(Fields::detached_entries(vec![
                scalar("port", Scalar::Int(1)).as_commented(),
            ])),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "svc:\n  - port: 9\n  - {}\n    #port: 1\n");
        let round = reparse(&out);
        let FieldKind::Value(value) = &round.get("svc").unwrap().kind else {
            panic!("svc should be an attribute value");
        };
        let ValueKind::Seq(elements) = &value.kind else {
            panic!("svc should reparse as a sequence");
        };
        assert_eq!(elements.len(), 2, "the commented element must survive");
    }

    #[test]
    fn emit_yaml_groups_repeated_blocks_into_one_block_sequence() {
        // Arrange
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![block(1), block(2)]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "service:\n  - port: 1\n  - port: 2\n");
        reparse(&out);
    }

    #[test]
    fn emit_yaml_writes_an_active_empty_block_inline() {
        // Arrange
        // A block whose entries are all commented has nothing active to write
        // below `key:`, and comment lines alone read back as null. It renders
        // `{}` the way a commented block does, so the level reads as an empty
        // mapping.
        let inner =
            Fields::detached_entries(vec![scalar("max_body_mb", Scalar::Int(16)).as_commented()]);
        let fields = Fields::detached(vec![Field::detached_block("limits", inner)]);

        // Act
        let out = emit_yaml(&fields).expect("emit yaml");

        // Assert
        assert_eq!(out, "limits: {}\n");
        let reparsed = reparse(&out);
        let FieldKind::Value(value) = &reparsed.get("limits").unwrap().kind else {
            panic!("limits should read back as a mapping value");
        };
        assert!(matches!(&value.kind, ValueKind::Map(level) if level.iter().count() == 0));
    }

    #[test]
    fn emit_yaml_rejects_a_key_longer_than_the_simple_key_limit() {
        // Arrange
        // YAML caps a simple key at 1024 characters, so a longer name has no
        // written form the parser reads back.
        let long = "k".repeat(1100);
        let fields = Fields::detached(vec![scalar(&long, Scalar::Int(1))]);

        // Act
        let result = emit_yaml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableName {
                name: long,
                path: String::new(),
            })
        );
    }

    #[test]
    fn emit_yaml_writes_a_key_at_the_simple_key_limit() {
        // Arrange
        let long = "k".repeat(900);
        let fields = Fields::detached(vec![scalar(&long, Scalar::Int(1))]);

        // Act
        let out = emit_yaml(&fields).expect("emit yaml");

        // Assert
        let reparsed = reparse(&out);
        assert!(reparsed.get(&long).is_some());
    }

    #[test]
    fn emit_yaml_rejects_a_native_label_it_cannot_write() {
        // Arrange
        // A parsed HCL or KDL block carries its label on the inner level, and
        // YAML has no label syntax and no field name to write it with.
        let inner = Fields::detached(vec![scalar("host", Scalar::String("h".to_string()))])
            .with_label(crate::source::Located::detached("api".to_string()));
        let fields = Fields::detached(vec![Field::detached_block("upstream", inner)]);

        // Act
        let result = emit_yaml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableLabel {
                label: "api".to_string(),
                path: "upstream".to_string(),
            })
        );
    }

    #[test]
    fn emit_yaml_rejects_a_value_beside_a_same_named_block() {
        // Arrange
        let inner = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![scalar("y", Scalar::Int(2))])),
        ]);
        let fields = Fields::detached(vec![Field::detached_block("tls", inner)]);

        // Act
        let result = emit_yaml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "tls".to_string(),
            })
        );
    }

    #[test]
    fn emit_yaml_rejects_an_unrepresentable_value_with_its_dotted_path() {
        // Arrange
        let inner = Fields::detached(vec![Field::detached_value(
            "when",
            Value::detached(ValueKind::Other("alias")),
        )]);
        let fields = Fields::detached(vec![Field::detached_block("tls", inner)]);

        // Act
        let result = emit_yaml(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "alias",
                path: "tls.when".to_string(),
            })
        );
    }

    #[test]
    fn emit_yaml_round_trips_non_finite_floats() {
        // YAML 1.2 writes all three natively, so YAML joins TOML and KDL where
        // JSON and HCL refuse.
        for (value, written) in [
            (f64::INFINITY, ".inf"),
            (f64::NEG_INFINITY, "-.inf"),
            (f64::NAN, ".nan"),
        ] {
            // Arrange
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(value))]);

            // Act
            let out = emit_yaml(&fields).unwrap();

            // Assert
            assert_eq!(out, format!("rate: {written}\n"));
            let round = reparse(&out);
            let mut report = Report::new();
            let parsed = parse_float_field(round.get("rate").unwrap(), &mut report).unwrap();
            if value.is_nan() {
                assert!(parsed.value.is_nan());
            } else {
                assert_eq!(parsed.value, value);
            }
            assert!(!report.has_issues());
        }
    }

    #[test]
    fn emit_yaml_keeps_the_float_form() {
        // Arrange
        // The debug form of a finite float always carries a point or an
        // exponent, so the core schema reads it back as a float.
        let fields = Fields::detached(vec![
            scalar("whole", Scalar::Float(1.0)),
            scalar("large", Scalar::Float(1e20)),
            scalar("count", Scalar::Int(4)),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "whole: 1.0\nlarge: 1e20\ncount: 4\n");
        let round = reparse(&out);
        let mut report = Report::new();
        assert_eq!(
            parse_float_field(round.get("large").unwrap(), &mut report)
                .unwrap()
                .value,
            1e20
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_yaml_quotes_every_string_including_the_schema_traps() {
        // Arrange
        // Each of these resolves to something other than a string when written
        // plain, so quoting is what keeps the round trip honest.
        let traps = ["no", "yes", "true", "null", "~", "8080", "1.5", ".inf", ""];
        let fields = Fields::detached(
            traps
                .iter()
                .enumerate()
                .map(|(index, value)| text(&format!("k{index}"), value))
                .collect(),
        );

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        let round = reparse(&out);
        let mut report = Report::new();
        for (index, value) in traps.iter().enumerate() {
            let name = format!("k{index}");
            assert_eq!(
                parse_string_field(round.get(&name).unwrap(), &mut report)
                    .unwrap()
                    .value,
                *value,
                "value {value:?} should read back as a string"
            );
        }
        assert!(!report.has_issues(), "{:?}", report.issues());
    }

    #[test]
    fn emit_yaml_escapes_the_quote_the_backslash_and_the_controls() {
        // Arrange
        let hostile = "quote\" backslash\\ nl\n tab\t cr\r bs\u{8} ff\u{c} \
                       nul\u{0} unit\u{1f} snowman\u{2603}";
        let fields = Fields::detached(vec![text("greeting", hostile)]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert!(out.contains("\\\"") && out.contains("\\\\"), "got: {out}");
        assert!(out.contains("snowman\u{2603}"), "raw UTF-8 passes through");
        let round = reparse(&out);
        let mut report = Report::new();
        assert_eq!(
            parse_string_field(round.get("greeting").unwrap(), &mut report)
                .unwrap()
                .value,
            hostile
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_yaml_writes_a_plain_key_beside_a_quoted_one() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("max_body-mb", Scalar::Int(1)),
            scalar("weird key", Scalar::Int(2)),
            scalar("9lives", Scalar::Int(3)),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "max_body-mb: 1\n\"weird key\": 2\n\"9lives\": 3\n");
        let round = reparse(&out);
        for name in ["max_body-mb", "weird key", "9lives"] {
            assert!(round.get(name).is_some(), "key {name} should read back");
        }
    }

    #[test]
    fn emit_yaml_writes_an_unparsed_scalar_as_a_string() {
        // Arrange
        // A layered tree carries unparsed text from an environment variable or
        // a flag, whose type was never decided.
        let fields = Fields::detached(vec![scalar("port", Scalar::Unparsed("8080".to_string()))]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "port: \"8080\"\n");
    }

    #[test]
    fn emit_yaml_renders_doc_comments_above_their_entries() {
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
        let out = emit_yaml(&fields).unwrap();

        // Assert
        // The blank line goes above the block's comment, and a nested comment
        // carries its field's indentation.
        assert_eq!(
            out,
            "# The port.\nport: 1\n\n# Request limits.\nlimits:\n  # Max body size.\n  max_body_mb: 16\n"
        );
        reparse(&out);
    }

    #[test]
    fn emit_yaml_renders_every_commented_shape_behind_a_spaceless_marker() {
        // Arrange
        // The four shapes a template hides: an optional leaf, an optional
        // string list, an unmarked optional block, and an empty repeated block.
        let repeated = Value::detached(ValueKind::Seq(vec![Value::detached(map(vec![]))]));
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            text("pid_file", "").as_commented(),
            seq("allow", vec![]).as_commented(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
            Field::detached_value("svc", repeated).as_commented(),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        // The block field sorts after the values, and only the block-rendered
        // member takes a blank line.
        assert_eq!(
            out,
            "port: 8080\n#pid_file: \"\"\n#allow: []\n\n#svc:\n  #- {}\n#tls: {}\n"
        );
    }

    #[test]
    fn emit_yaml_puts_a_nested_commented_entry_after_its_indentation() {
        // Arrange
        // Deleting the `#` must leave the entry at its own column.
        let inner = Fields::detached_entries(vec![
            text("mode", "log").into(),
            scalar("retry", Scalar::Int(0)).as_commented(),
        ]);
        let fields = Fields::detached(vec![Field::detached_block("limits", inner)]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "limits:\n  mode: \"log\"\n  #retry: 0\n");
    }

    #[test]
    fn emit_yaml_reparses_a_template_to_the_active_fields_alone() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            text("pid_file", "")
                .with_doc(Some("The PID file path.".to_string()))
                .as_commented(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        // A doc above a commented entry is already a comment, so it renders
        // once rather than behind a second marker.
        assert!(
            out.contains("# The PID file path.\n#pid_file"),
            "got:\n{out}"
        );
        assert!(!out.contains("##"), "got:\n{out}");
        let round = reparse(&out);
        let names: Vec<&str> = round.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["port"]);
    }

    #[test]
    fn emit_yaml_uncomments_to_an_empty_instance_of_each_shape() {
        // Arrange
        // Uncommenting is deleting the `#`, and what remains must parse to the
        // shape the field declares.
        let repeated = Value::detached(ValueKind::Seq(vec![Value::detached(map(vec![]))]));
        let fields = Fields::detached_entries(vec![
            text("pid_file", "").as_commented(),
            seq("allow", vec![]).as_commented(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
            Field::detached_value("svc", repeated).as_commented(),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        // The template as emitted, before any uncommenting, parses as a
        // configuration that sets nothing.
        assert_eq!(reparse(&out).iter().count(), 0);
        let uncommented = out.replace('#', "");
        let round = reparse(&uncommented);
        let mut report = Report::new();
        assert_eq!(
            parse_string_field(round.get("pid_file").unwrap(), &mut report)
                .unwrap()
                .value,
            ""
        );
        assert!(
            parse_string_list_field(round.get("allow").unwrap(), &mut report)
                .unwrap()
                .value
                .is_empty()
        );
        let FieldKind::Value(value) = &round.get("svc").unwrap().kind else {
            panic!("svc should be an attribute value");
        };
        let ValueKind::Seq(elements) = &value.kind else {
            panic!("svc should uncomment to a sequence, got {:?}", value.kind);
        };
        assert_eq!(elements.len(), 1);
        assert!(matches!(elements[0].kind, ValueKind::Map(_)));
        assert!(!report.has_issues(), "{:?}", report.issues());
    }

    #[test]
    fn emit_yaml_excludes_commented_fields_from_grouping() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("x", Scalar::Int(1)).into(),
            scalar("x", Scalar::Int(2)).as_commented(),
        ]);

        // Act
        let out = emit_yaml(&fields).unwrap();

        // Assert
        assert_eq!(out, "x: 1\n#x: 2\n");
    }
}
