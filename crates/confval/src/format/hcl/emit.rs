//! HCL write path: serializes a neutral [`Fields`] tree to canonical HCL.
//!
//! This is the inverse of [`parse_hcl_fields`](super::parse_hcl_fields). It
//! builds an `hcl-edit` `Body` by structure, indents each nesting level, and
//! renders the doc comments an annotated template carries.

use super::commented::commented_text;
use crate::format::EmitError;
use crate::format::emit::{
    child_path, comment_lines, first_conflicting_name, repeated_name, values_then_blocks,
};
use crate::format::field::{FieldKind, Fields, Scalar, Value, ValueKind};
use hcl_edit::Decorate;
use hcl_edit::Ident;
use hcl_edit::expr::{Array, Expression, Object, ObjectKey, ObjectValue};
use hcl_edit::structure::{Attribute, Block, Body, Structure};

/// Serializes a [`Fields`] tree to canonical HCL text.
///
/// This is the inverse of [`parse_hcl_fields`](super::parse_hcl_fields). It
/// builds an `hcl-edit` `Body` by structure and returns its text, dropping the
/// comments and layout the neutral model never held. A nested struct emits as a
/// block, a repeated block emits once per element, and a non-identifier object
/// key is quoted. Values emit before blocks at each level, each group in
/// declaration order, with a blank line above every block that follows another
/// structure. It fails on a non-identifier attribute or block name, which
/// HCL cannot spell, on a [`ValueKind::Other`], on two same-named values at one
/// level, which HCL rejects as duplicate attributes, and on any repeated name
/// inside an object. Those arise only when you emit a parsed or hand-built
/// `Fields`, not on the populate path. It also fails on the two numeric values
/// HCL has no literal for, an `i64::MIN` and a non-finite float, which a
/// populated spec can hold.
pub fn emit_hcl(fields: &Fields) -> Result<String, EmitError> {
    let (mut body, pending) = emit_body(fields, 0, "")?;
    if !pending.is_empty() {
        body.decor_mut().set_suffix(pending);
    }
    let text = body.to_string();
    // A commented block that opens the document carries a blank line for the
    // structure it would follow. At the top nothing precedes it.
    match text.strip_prefix('\n') {
        Some(stripped) => Ok(stripped.to_string()),
        None => Ok(text),
    }
}

/// Builds a `Body` indented for the given nesting level.
///
/// Each structure is prefixed with `level` steps of two spaces. A block's inner
/// body carries a suffix of the block's own indent, which `hcl-edit` writes just
/// before the closing brace, so the brace lines up with the opener. Without the
/// suffix the brace would be at column zero.
///
/// Values emit before blocks at each level, each group in declaration order,
/// and a blank line separates every block from the structure above it. This is
/// the Terraform layout convention, and it matches the order TOML is forced
/// into by its syntax, where a bare key after a table header would belong to
/// that table.
/// Returns the body and the commented-out text still pending at the level's
/// end. The caller attaches the pending text as the enclosing body's decor
/// suffix, so it renders before the closing brace, or at the document's end at
/// the root.
pub(super) fn emit_body(
    fields: &Fields,
    level: usize,
    path: &str,
) -> Result<(Body, String), EmitError> {
    // HCL repeats blocks freely and spells a value next to a block, but it
    // rejects a duplicate attribute, and hcl-edit would keep only the first.
    if let Some(name) = duplicate_attribute_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let indent = "  ".repeat(level);
    let mut body = Body::new();
    let mut pending = String::new();
    let mut emitted = 0usize;
    for entry in values_then_blocks(fields) {
        let field = entry.field();
        if entry.is_commented() {
            pending.push_str(&commented_text(field, level, path)?);
            continue;
        }
        // The prefix carries any pending commented text, then the field's doc
        // comment above its indentation, so the comment aligns with the field
        // it documents.
        let pending_text = std::mem::take(&mut pending);
        let doc_prefix = hcl_comment_prefix(&field.doc, &indent);
        match &field.kind {
            FieldKind::Value(value) => {
                let child = child_path(path, &field.name);
                let mut attribute =
                    Attribute::new(ident_of(&field.name, path)?, hcl_expr_of(value, &child)?);
                attribute
                    .decor_mut()
                    .set_prefix(format!("{pending_text}{doc_prefix}"));
                body.push(Structure::Attribute(attribute));
            }
            FieldKind::Block(inner) => {
                let child = child_path(path, &field.name);
                let mut block = Block::new(ident_of(&field.name, path)?);
                // The blank line separates the block from the structure above
                // it, so it follows any pending commented text rather than
                // preceding it. A commented entry belongs to the value group it
                // was written after, the way TOML and KDL render it.
                let separator = if emitted == 0 { "" } else { "\n" };
                block
                    .decor_mut()
                    .set_prefix(format!("{pending_text}{separator}{doc_prefix}"));
                let (mut inner_body, inner_pending) = emit_body(inner, level + 1, &child)?;
                inner_body
                    .decor_mut()
                    .set_suffix(format!("{inner_pending}{indent}"));
                block.body = inner_body;
                body.push(Structure::Block(block));
            }
        }
        emitted += 1;
    }
    Ok((body, pending))
}

/// The decor prefix for a field: its doc comment as `# line` comments, each at
/// the field's indentation, followed by the field's own indent. With no doc it
/// is the indent alone.
fn hcl_comment_prefix(doc: &Option<String>, indent: &str) -> String {
    match doc {
        Some(text) => {
            let mut out = String::new();
            for line in comment_lines(text) {
                out.push_str(indent);
                if line.is_empty() {
                    out.push_str("#\n");
                } else {
                    out.push_str("# ");
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            out.push_str(indent);
            out
        }
        None => indent.to_string(),
    }
}

fn hcl_expr_of(value: &Value, path: &str) -> Result<Expression, EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => hcl_expr_of_scalar(scalar, path),
        ValueKind::Seq(elements) => {
            let mut array = Array::new();
            for element in elements {
                array.push(hcl_expr_of(element, path)?);
            }
            Ok(Expression::Array(array))
        }
        ValueKind::Map(inner) => Ok(Expression::Object(hcl_object_of(inner, path)?)),
        ValueKind::Other(label) => Err(EmitError::UnrepresentableValue {
            label,
            path: path.to_string(),
        }),
    }
}

/// A name used by more than one active value field at one level, which HCL
/// cannot spell as duplicate attributes. HCL repeats blocks freely, so only the
/// value fields in a group count.
fn duplicate_attribute_name(fields: &Fields) -> Option<&str> {
    first_conflicting_name(fields, |group| {
        group
            .iter()
            .filter(|field| matches!(field.kind, FieldKind::Value(_)))
            .count()
            > 1
    })
}

fn hcl_object_of(fields: &Fields, path: &str) -> Result<Object, EmitError> {
    if let Some(name) = repeated_name(fields) {
        return Err(EmitError::ConflictingName {
            name: name.to_string(),
            path: path.to_string(),
        });
    }
    let mut object = Object::new();
    // An inline object has no comment spelling, and `iter` yields no commented
    // entry, so one renders nothing here.
    for field in fields.iter() {
        let child = child_path(path, &field.name);
        let value = match &field.kind {
            FieldKind::Value(value) => hcl_expr_of(value, &child)?,
            FieldKind::Block(inner) => Expression::Object(hcl_object_of(inner, &child)?),
        };
        object.insert(object_key_of(&field.name), ObjectValue::new(value));
    }
    Ok(object)
}

/// An identifier object key stays bare, and a non-identifier key is quoted as a
/// string expression, which HCL represents natively.
fn object_key_of(name: &str) -> ObjectKey {
    match Ident::try_new(name) {
        Ok(ident) => ObjectKey::Ident(ident.into()),
        Err(_) => ObjectKey::Expression(Expression::from(name.to_string())),
    }
}

fn hcl_expr_of_scalar(scalar: &Scalar, path: &str) -> Result<Expression, EmitError> {
    let expr = match scalar {
        Scalar::String(string) => Expression::from(string.clone()),
        Scalar::Int(int) => {
            // HCL reads a negative integer as a negation of its magnitude, and
            // i64::MIN's magnitude is 2^63, which overflows i64 on the way back
            // in. HCL has no literal that round-trips it, so refuse rather than
            // emit text the HCL parser cannot read. The upstream fix is
            // https://github.com/martinohmann/hcl-rs/pull/549. Once a released
            // hcl-edit round-trips i64::MIN, this rejection can be removed.
            if *int == i64::MIN {
                return Err(EmitError::UnrepresentableValue {
                    label: "i64::MIN",
                    path: path.to_string(),
                });
            }
            Expression::from(*int)
        }
        Scalar::Float(float) => {
            // HCL has no literal for infinity or NaN. hcl-edit maps a non-finite
            // float to `null`, so refuse rather than silently change the value.
            if !float.is_finite() {
                return Err(EmitError::UnrepresentableValue {
                    label: "non-finite float",
                    path: path.to_string(),
                });
            }
            // hcl-edit's own float conversion turns a whole-valued float into
            // an integer with a saturating cast, which corrupts a magnitude of
            // 2^63 or more and drops the float spelling everywhere else.
            // Parsing the float's shortest round-trip text instead keeps the
            // emitted literal exact, because a parsed expression renders its
            // own text verbatim.
            match format!("{float:?}").parse::<Expression>() {
                Ok(expression) => expression,
                // Unreachable: the debug form of a finite float is always a
                // valid HCL number or a negation of one.
                Err(_) => {
                    return Err(EmitError::UnrepresentableValue {
                        label: "float",
                        path: path.to_string(),
                    });
                }
            }
        }
        Scalar::Bool(boolean) => Expression::from(*boolean),
        Scalar::Unparsed(raw) => Expression::from(raw.clone()),
    };
    Ok(expr)
}

/// An attribute or block name must be a valid HCL identifier, because HCL has no
/// quoted spelling for one. A non-identifier name is unrepresentable.
fn ident_of(name: &str, path: &str) -> Result<Ident, EmitError> {
    Ident::try_new(name).map_err(|_| EmitError::UnrepresentableName {
        name: name.to_string(),
        path: path.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::parse_hcl_fields;
    use super::*;
    use crate::diagnostic::Report;
    use crate::format::field::{Field, FromFields};
    use crate::format::parse::{
        parse_float_field, parse_int_field, parse_string_field, parse_struct_list_field,
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

    fn reparse(text: &str) -> Fields {
        let mut sources = SourceMap::new();
        let id = sources.add("emitted.hcl", text.to_string());
        let mut report = Report::new();
        let fields = parse_hcl_fields(&sources, id, &mut report).unwrap();
        assert!(
            !report.has_issues(),
            "reparse issues: {:?}",
            report.issues()
        );
        fields
    }

    #[test]
    fn emit_hcl_writes_a_commented_leaf_after_the_active_values() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n#pid_file = \"\"\n");
    }

    #[test]
    fn emit_hcl_renders_a_doc_above_its_commented_entry() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("pid_file", Scalar::String(String::new()))
                .with_doc(Some("The PID file path.".to_string()))
                .as_commented(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "port = 8080\n# The PID file path.\n#pid_file = \"\"\n"
        );
    }

    #[test]
    fn emit_hcl_writes_a_commented_empty_block() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n\n#tls {\n#}\n");
    }

    #[test]
    fn emit_hcl_writes_a_commented_list_hint_as_a_block() {
        // Arrange
        // The nested-list shape spells the repeated-block form, the same
        // spelling an active repeated block has.
        let hint = Value::detached(ValueKind::Seq(vec![Value::detached(ValueKind::Map(
            Fields::detached(vec![]),
        ))]));
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            Field::detached_value("svc", hint).as_commented(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n\n#svc {\n#}\n");
    }

    #[test]
    fn emit_hcl_indents_a_commented_entry_inside_a_block() {
        // Arrange
        let inner = Fields::detached_entries(vec![
            scalar("mode", Scalar::String("log".to_string())).into(),
            scalar("rate", Scalar::Int(0)).as_commented(),
        ]);
        let fields = Fields::detached(vec![Field::detached_block("limits", inner)]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // The marker follows the level's indent, so the entry lines up with the
        // body around it and uncommenting restores the aligned entry.
        assert_eq!(text, "limits {\n  mode = \"log\"\n  #rate = 0\n}\n");
    }

    #[test]
    fn emit_hcl_attaches_a_commented_entry_above_a_doc_commented_block() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            )
            .with_doc(Some("Request limits.".to_string()))
            .into(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "#pid_file = \"\"\n# Request limits.\nlimits {\n  max_body_mb = 16\n}\n"
        );
    }

    #[test]
    fn emit_hcl_keeps_a_commented_entry_with_the_values_above_it() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
            Field::detached_block(
                "limits",
                Fields::detached(vec![scalar("max_body_mb", Scalar::Int(16))]),
            )
            .with_doc(Some("Request limits.".to_string()))
            .into(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // The blank line separates the block from the value group, so the
        // commented entry stays with the values it belongs to.
        assert_eq!(
            text,
            "port = 8080\n#pid_file = \"\"\n\n# Request limits.\nlimits {\n  max_body_mb = 16\n}\n"
        );
    }

    #[test]
    fn emit_hcl_renders_adjacent_commented_entries_in_order() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("a", Scalar::Int(1)).as_commented(),
            scalar("b", Scalar::Int(2)).as_commented(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 8080\n#a = 1\n#b = 2\n");
    }

    #[test]
    fn emit_hcl_renders_an_all_commented_block_inside_its_braces() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_block(
            "limits",
            Fields::detached_entries(vec![scalar("max_body_mb", Scalar::Int(16)).as_commented()]),
        )]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "limits {\n  #max_body_mb = 16\n}\n");
        reparse(&text);
    }

    #[test]
    fn emit_hcl_excludes_commented_fields_from_the_duplicate_check() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("x", Scalar::Int(1)).into(),
            scalar("x", Scalar::Int(2)).as_commented(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "x = 1\n#x = 2\n");
    }

    #[test]
    fn emit_hcl_drops_a_commented_field_inside_an_object() {
        // Arrange
        let map = Fields::detached_entries(vec![
            scalar("cert", Scalar::String("a.pem".to_string())).into(),
            scalar("key", Scalar::String(String::new())).as_commented(),
        ]);
        let fields = Fields::detached(vec![Field::detached_value(
            "tls",
            Value::detached(ValueKind::Map(map)),
        )]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert!(text.contains("cert"), "got: {text:?}");
        assert!(!text.contains("key"), "got: {text:?}");
    }

    #[test]
    fn emit_hcl_reparses_a_commented_template_to_the_active_fields_alone() {
        // Arrange
        let fields = Fields::detached_entries(vec![
            scalar("port", Scalar::Int(8080)).into(),
            scalar("pid_file", Scalar::String(String::new())).as_commented(),
            Field::detached_block("tls", Fields::detached(vec![])).as_commented(),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        let round = reparse(&text);
        let names: Vec<&str> = round.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["port"]);
    }

    #[test]
    fn emit_hcl_writes_canonical_text() {
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
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // The block body is indented one level, the closing brace lines up
        // with the opener, and a blank line separates the block from the
        // attribute above it.
        assert_eq!(
            text,
            "hostname = \"api\"\nport = 8080\n\nlimits {\n  max_body_mb = 16\n}\n"
        );
    }

    #[test]
    fn emit_hcl_starts_a_leading_block_without_a_blank_line() {
        // Arrange
        // The blank line separates a block from what precedes it. A block that
        // opens the document, or opens its parent's body, has nothing above it.
        let inner = Fields::detached(vec![Field::detached_block(
            "burst",
            Fields::detached(vec![scalar("rate", Scalar::Int(100))]),
        )]);
        let fields = Fields::detached(vec![Field::detached_block("limits", inner)]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "limits {\n  burst {\n    rate = 100\n  }\n}\n");
    }

    #[test]
    fn emit_hcl_separates_consecutive_blocks_with_a_blank_line() {
        // Arrange
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![block(1), block(2)]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "service {\n  port = 1\n}\n\nservice {\n  port = 2\n}\n"
        );
    }

    #[test]
    fn emit_hcl_separates_a_nested_block_from_a_preceding_attribute() {
        // Arrange
        let inner = Fields::detached(vec![
            scalar("mode", Scalar::String("log".to_string())),
            Field::detached_block(
                "burst",
                Fields::detached(vec![scalar("rate", Scalar::Int(100))]),
            ),
        ]);
        let fields = Fields::detached(vec![Field::detached_block("limits", inner)]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // The blank line carries no indentation, and the nested block keeps its
        // own indent after it.
        assert_eq!(
            text,
            "limits {\n  mode = \"log\"\n\n  burst {\n    rate = 100\n  }\n}\n"
        );
    }

    #[test]
    fn emit_hcl_orders_values_before_blocks() {
        // Arrange
        // A value declared after a block still emits above it, matching the
        // Terraform convention and the order TOML is forced into by its syntax.
        let fields = Fields::detached(vec![
            Field::detached_block(
                "sprocket",
                Fields::detached(vec![scalar("max_height", Scalar::Int(32))]),
            ),
            scalar("max_weight", Scalar::Int(16)),
            Field::detached_block(
                "sprocket2",
                Fields::detached(vec![scalar("max_height", Scalar::Int(32))]),
            ),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "max_weight = 16\n\nsprocket {\n  max_height = 32\n}\n\nsprocket2 {\n  max_height = 32\n}\n"
        );
    }

    #[test]
    fn emit_hcl_keeps_repeated_block_order_across_an_interleaved_value() {
        // Arrange
        // Repeated blocks are list elements, so the partition must keep their
        // relative order while the value moves above them.
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![
            block(1),
            scalar("name", Scalar::String("x".to_string())),
            block(2),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(
            text,
            "name = \"x\"\n\nservice {\n  port = 1\n}\n\nservice {\n  port = 2\n}\n"
        );
    }

    #[test]
    fn emit_hcl_puts_the_blank_line_above_a_blocks_doc_comment() {
        // Arrange
        // The comment belongs to the block, so the separating blank line goes
        // above the comment, not between the comment and the block.
        let fields = Fields::detached(vec![
            scalar("port", Scalar::Int(1)),
            Field::detached_block("limits", Fields::detached(vec![]))
                .with_doc(Some("Request limits.".to_string())),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "port = 1\n\n# Request limits.\nlimits {\n}\n");
    }

    #[test]
    fn emit_hcl_round_trips_scalars_and_a_block() {
        let fields = Fields::detached(vec![
            scalar("name", Scalar::String("api".to_string())),
            scalar("count", Scalar::Int(42)),
            scalar("flag", Scalar::Bool(true)),
            Field::detached_block(
                "tls",
                Fields::detached(vec![scalar("cert", Scalar::String("a.pem".to_string()))]),
            ),
        ]);
        let round = reparse(&emit_hcl(&fields).unwrap());
        let mut report = Report::new();
        assert_eq!(
            parse_int_field(round.get("count").unwrap(), &mut report)
                .unwrap()
                .value,
            42
        );
        let FieldKind::Block(tls) = &round.get("tls").unwrap().kind else {
            panic!("tls should be a block");
        };
        assert_eq!(
            parse_string_field(tls.get("cert").unwrap(), &mut report)
                .unwrap()
                .value,
            "a.pem"
        );
    }

    #[test]
    fn emit_hcl_writes_a_nested_list_as_repeated_blocks() {
        let block = |port: i64| {
            Field::detached_block(
                "service",
                Fields::detached(vec![scalar("port", Scalar::Int(port))]),
            )
        };
        let fields = Fields::detached(vec![block(1), block(2)]);
        let text = emit_hcl(&fields).unwrap();
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
    fn emit_hcl_rejects_a_non_identifier_attribute_name() {
        let fields = Fields::detached(vec![scalar("weird key", Scalar::Int(1))]);
        assert_eq!(
            emit_hcl(&fields),
            Err(EmitError::UnrepresentableName {
                name: "weird key".to_string(),
                path: String::new(),
            })
        );
    }

    #[test]
    fn emit_hcl_quotes_a_non_identifier_object_key() {
        let map = Fields::detached(vec![scalar("a b", Scalar::Int(1))]);
        let fields = Fields::detached(vec![Field::detached_value(
            "obj",
            Value::detached(ValueKind::Map(map)),
        )]);
        let text = emit_hcl(&fields).unwrap();
        assert!(text.contains("\"a b\""), "got: {text}");
        let round = reparse(&text);
        let FieldKind::Value(value) = &round.get("obj").unwrap().kind else {
            panic!("obj should be an attribute");
        };
        let ValueKind::Map(inner) = &value.kind else {
            panic!("obj should be a map");
        };
        assert!(inner.get("a b").is_some());
    }

    #[test]
    fn emit_hcl_rejects_an_unrepresentable_value() {
        let fields = Fields::detached(vec![Field::detached_value(
            "name",
            Value::detached(ValueKind::Other("string template")),
        )]);
        assert_eq!(
            emit_hcl(&fields),
            Err(EmitError::UnrepresentableValue {
                label: "string template",
                path: "name".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_rejects_i64_min() {
        // i64::MIN emits as `-9223372036854775808`, which HCL reads as a negation
        // of 2^63 and overflows on the way back in, so emit must refuse it rather
        // than produce text the HCL parser cannot read.
        let fields = Fields::detached(vec![scalar("offset", Scalar::Int(i64::MIN))]);
        assert_eq!(
            emit_hcl(&fields),
            Err(EmitError::UnrepresentableValue {
                label: "i64::MIN",
                path: "offset".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_rejects_a_non_finite_float() {
        // HCL has no literal for infinity or NaN, and hcl-edit would emit `null`,
        // so emit must refuse rather than silently change the value.
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(value))]);
            assert_eq!(
                emit_hcl(&fields),
                Err(EmitError::UnrepresentableValue {
                    label: "non-finite float",
                    path: "rate".to_string(),
                }),
                "value {value} should be rejected"
            );
        }
    }

    #[test]
    fn emit_hcl_rejects_two_attributes_sharing_a_name() {
        // Arrange
        // HCL rejects a duplicate attribute at parse time, and hcl-edit keeps
        // only the first on emit, so refusing beats losing one silently.
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            scalar("x", Scalar::Int(2)),
        ]);

        // Act
        let result = emit_hcl(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_keeps_a_value_and_a_block_sharing_a_name() {
        // Arrange
        // HCL spells `x = 1` next to `x { }`, so the pair emits and reparses,
        // unlike in TOML where the same pair is refused.
        let fields = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            Field::detached_block("x", Fields::detached(vec![scalar("y", Scalar::Int(2))])),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert!(text.contains("x = 1"), "got: {text:?}");
        assert!(text.contains("x {"), "got: {text:?}");
        let round = reparse(&text);
        assert_eq!(round.iter().count(), 2);
    }

    #[test]
    fn emit_hcl_rejects_a_repeated_name_inside_an_object() {
        // Arrange
        // An object is a map, so a repeated key has no faithful spelling.
        let pair = Fields::detached(vec![
            scalar("x", Scalar::Int(1)),
            scalar("x", Scalar::Int(2)),
        ]);
        let fields = Fields::detached(vec![Field::detached_value(
            "obj",
            Value::detached(ValueKind::Map(pair)),
        )]);

        // Act
        let result = emit_hcl(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::ConflictingName {
                name: "x".to_string(),
                path: "obj".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_names_the_nested_path_in_an_error() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_block(
            "limits",
            Fields::detached(vec![scalar("rate", Scalar::Float(f64::NAN))]),
        )]);

        // Act
        let result = emit_hcl(&fields);

        // Assert
        assert_eq!(
            result,
            Err(EmitError::UnrepresentableValue {
                label: "non-finite float",
                path: "limits.rate".to_string(),
            })
        );
    }

    #[test]
    fn emit_hcl_writes_a_float_as_a_float_literal() {
        // Arrange
        let fields = Fields::detached(vec![
            scalar("whole", Scalar::Float(4.0)),
            scalar("fractional", Scalar::Float(1.5)),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // A whole-valued float keeps a float spelling, so the neutral model's
        // float kind survives the reparse instead of collapsing to an integer.
        assert_eq!(text, "whole = 4.0\nfractional = 1.5\n");
        let round = reparse(&text);
        for name in ["whole", "fractional"] {
            let FieldKind::Value(value) = &round.get(name).unwrap().kind else {
                panic!("{name} should be an attribute");
            };
            assert!(
                matches!(value.kind, ValueKind::Scalar(Scalar::Float(_))),
                "{name} should reparse as a float, got: {:?}",
                value.kind
            );
        }
    }

    #[test]
    fn emit_hcl_round_trips_a_float_beyond_i64_range() {
        // A whole float of magnitude 2^63 or more has no exact i64, so an
        // integer collapse would saturate and corrupt the value.
        let extremes = [1e19, -1e300, 9_223_372_036_854_775_808.0, 1.5e300];
        for expected in extremes {
            // Arrange
            let fields = Fields::detached(vec![scalar("rate", Scalar::Float(expected))]);

            // Act
            let text = emit_hcl(&fields).unwrap();

            // Assert
            let round = reparse(&text);
            let mut report = Report::new();
            let parsed = parse_float_field(round.get("rate").unwrap(), &mut report).unwrap();
            assert_eq!(parsed.value, expected, "emitted text: {text:?}");
            assert!(!report.has_issues());
        }
    }

    #[test]
    fn emit_hcl_round_trips_an_adversarial_string() {
        // Arrange
        // Escaping goes through hcl-edit, so this guards the crate against a
        // regression in how quotes, backslashes, line breaks, tabs, unicode,
        // and control characters are spelled.
        let hostile = "quote\" backslash\\ newline\n tab\t snowman\u{2603} del\u{7f} bel\u{7}";
        let fields = Fields::detached(vec![scalar(
            "greeting",
            Scalar::String(hostile.to_string()),
        )]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        let round = reparse(&text);
        let mut report = Report::new();
        let parsed = parse_string_field(round.get("greeting").unwrap(), &mut report).unwrap();
        assert_eq!(parsed.value, hostile, "emitted: {text:?}");
        assert!(!report.has_issues());
    }

    #[test]
    fn emit_hcl_writes_an_empty_block_that_reparses() {
        // Arrange
        let fields = Fields::detached(vec![Field::detached_block(
            "empty",
            Fields::detached(vec![]),
        )]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        assert_eq!(text, "empty {\n}\n");
        let round = reparse(&text);
        assert!(matches!(
            round.get("empty").unwrap().kind,
            FieldKind::Block(_)
        ));
    }

    #[test]
    fn emit_hcl_normalizes_a_control_char_in_a_doc_comment() {
        // Arrange
        // A lone carriage return in a doc override would end an HCL comment early.
        let fields = Fields::detached(vec![
            scalar("port", Scalar::Int(1)).with_doc(Some("line one\rline two".to_string())),
        ]);

        // Act
        let text = emit_hcl(&fields).unwrap();

        // Assert
        // The carriage return became a second comment line, so the template reparses.
        assert!(text.contains("# line one\n"), "got: {text:?}");
        assert!(text.contains("# line two\n"), "got: {text:?}");
        reparse(&text);
    }
}
