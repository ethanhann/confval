//! YAML write path: serializes a neutral [`Fields`] tree to canonical YAML.
//!
//! This is the inverse of [`parse_yaml_fields`](super::parse_yaml_fields). It
//! writes the text directly, because YAML's layout is small enough to state in
//! one place: block style throughout, two-space indentation, one entry per
//! line, a blank line above every member that renders as a block, and a
//! trailing newline. Three things render flow: a sequence whose elements are
//! all scalars, an empty sequence, and an empty mapping.

use crate::format::EmitError;
use crate::format::emit::{child_path, comment_lines, first_conflicting_name, values_then_blocks};
use crate::format::field::{Field, FieldKind, Fields, Scalar, Value, ValueKind};

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
/// field name. [`EmitError::UnrepresentableName`] therefore never arises.
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

/// One member of an emitted mapping: the same-named fields sharing its name.
enum Member<'a> {
    Values(Vec<&'a Value>),
    Blocks(Vec<&'a Fields>),
}

/// How a member renders, which decides its layout.
///
/// The distinction turns on the rendering rather than on the field's kind. A
/// kind-keyed rule would make emit non-idempotent, because a parse of the
/// emitted text yields `Map` values where the original held blocks.
#[derive(PartialEq, Clone, Copy)]
enum Shape {
    /// The whole member fits after `key: `.
    Inline,
    /// The member's body renders on the lines below `key:`.
    Block,
}

/// One member ready to render, with the annotation a template carries.
struct Rendered<'a> {
    name: &'a str,
    doc: Option<&'a str>,
    commented: bool,
    member: Member<'a>,
}

/// A name used at one level both as a value and as a block. YAML's only form
/// for the pair is a duplicate key, so emit refuses rather than losing one of
/// them. A commented entry is comment text, so it conflicts with nothing.
fn conflicting_name(fields: &Fields) -> Option<&str> {
    first_conflicting_name(fields, |group| {
        group
            .iter()
            .any(|field| matches!(field.kind, FieldKind::Value(_)))
            && group
                .iter()
                .any(|field| matches!(field.kind, FieldKind::Block(_)))
    })
}

/// The members of one level, values before blocks, each group at its first
/// occurrence's position. A commented entry stands alone, so it never joins a
/// group and never blocks an active field.
fn members_of(fields: &Fields) -> Vec<Rendered<'_>> {
    let mut members: Vec<Rendered> = Vec::new();
    let mut grouped: Vec<&str> = Vec::new();
    for entry in values_then_blocks(fields) {
        let field = entry.field();
        if entry.is_commented() {
            members.push(Rendered {
                name: &field.name,
                doc: field.doc.as_deref(),
                commented: true,
                member: lone(field),
            });
            continue;
        }
        if grouped.contains(&field.name.as_str()) {
            continue;
        }
        grouped.push(&field.name);
        let group: Vec<&Field> = fields
            .iter()
            .filter(|other| other.name == field.name)
            .collect();
        let member = match field.kind {
            FieldKind::Value(_) => Member::Values(
                group
                    .iter()
                    .filter_map(|other| match &other.kind {
                        FieldKind::Value(value) => Some(value),
                        FieldKind::Block(_) => None,
                    })
                    .collect(),
            ),
            FieldKind::Block(_) => Member::Blocks(
                group
                    .iter()
                    .filter_map(|other| match &other.kind {
                        FieldKind::Block(inner) => Some(inner),
                        FieldKind::Value(_) => None,
                    })
                    .collect(),
            ),
        };
        members.push(Rendered {
            name: &field.name,
            // Only one comment renders above the grouped member, so the group
            // takes the first doc any of its fields carries.
            doc: group.iter().find_map(|other| other.doc.as_deref()),
            commented: false,
            member,
        });
    }
    members
}

/// The member one field forms on its own.
fn lone(field: &Field) -> Member<'_> {
    match &field.kind {
        FieldKind::Value(value) => Member::Values(vec![value]),
        FieldKind::Block(inner) => Member::Blocks(vec![inner]),
    }
}

/// Writes one mapping level, each entry at `level`.
fn write_level(
    out: &mut String,
    fields: &Fields,
    level: usize,
    path: &str,
) -> Result<(), EmitError> {
    if let Some(name) = conflicting_name(fields) {
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
    let shape = shape_of(&rendered.member);
    if follows && shape == Shape::Block {
        out.push('\n');
    }
    write_doc(out, rendered.doc, level);
    let path = child_path(path, rendered.name);
    let mut body = String::new();
    indent(&mut body, level);
    write_key(&mut body, rendered.name);
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

/// How a member renders. A lone scalar, an empty collection, and a sequence of
/// scalars all fit on the key's line.
fn shape_of(member: &Member) -> Shape {
    match member {
        Member::Values(group) => match group.as_slice() {
            [only] => shape_of_value(only),
            _ => shape_of_elements(&flattened(group)),
        },
        Member::Blocks(group) => match group.as_slice() {
            [only] if only.entries().len() == 0 => Shape::Inline,
            [_] => Shape::Block,
            _ => Shape::Block,
        },
    }
}

fn shape_of_value(value: &Value) -> Shape {
    match &value.kind {
        ValueKind::Seq(elements) => {
            shape_of_elements(&elements.iter().collect::<Vec<_>>())
        }
        ValueKind::Map(inner) => {
            if inner.entries().len() == 0 {
                Shape::Inline
            } else {
                Shape::Block
            }
        }
        ValueKind::Scalar(_) | ValueKind::Other(_) => Shape::Inline,
    }
}

/// A sequence renders flow when it is empty or holds scalars alone.
fn shape_of_elements(elements: &[&Value]) -> Shape {
    let structural = elements
        .iter()
        .any(|element| matches!(element.kind, ValueKind::Map(_) | ValueKind::Seq(_)));
    if structural { Shape::Block } else { Shape::Inline }
}

/// Writes a member that fits after `key: `.
fn write_inline(out: &mut String, member: &Member, path: &str) -> Result<(), EmitError> {
    match member {
        Member::Values(group) => match group.as_slice() {
            [only] => write_value_inline(out, only, path),
            _ => write_flow(out, &flattened(group), path),
        },
        Member::Blocks(_) => {
            out.push_str("{}");
            Ok(())
        }
    }
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
            _ => write_sequence(out, &flattened(group), level + 1, path),
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

/// The elements a repeated value field contributes to its grouped sequence. A
/// sequence occurrence contributes its elements in document order, and a scalar
/// or a mapping contributes itself as one element. This is the accumulation the
/// walk performs, so a list-shaped field reads the same resolved list either
/// way.
fn flattened<'a>(group: &[&'a Value]) -> Vec<&'a Value> {
    let mut elements: Vec<&Value> = Vec::new();
    for value in group {
        match &value.kind {
            ValueKind::Seq(inner) => elements.extend(inner.iter()),
            _ => elements.push(value),
        }
    }
    elements
}

/// Writes one value on the key's line.
fn write_value_inline(out: &mut String, value: &Value, path: &str) -> Result<(), EmitError> {
    match &value.kind {
        ValueKind::Scalar(scalar) => {
            write_scalar(out, scalar);
            Ok(())
        }
        ValueKind::Seq(elements) => {
            write_flow(out, &elements.iter().collect::<Vec<_>>(), path)
        }
        ValueKind::Map(_) => {
            out.push_str("{}");
            Ok(())
        }
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
        // A scalar and an `Other` are never block-shaped, so this arm exists
        // only to keep the match total.
        _ => write_value_inline(out, value, path),
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

/// Writes a rendered element body with its first content line carrying the `- `
/// marker, so a mapping element opens on the marker's line.
///
/// A doc comment above that first entry keeps its own indentation and renders
/// before the marker, because a marker inside a comment would hide the element.
fn splice_dash(out: &mut String, body: &str, level: usize) {
    let column = level * 2;
    let mut spliced = false;
    for line in body.lines() {
        let content = line.trim_start();
        if !spliced && !content.is_empty() && !content.starts_with('#') && line.len() >= column + 2 {
            out.push_str(&line[..column]);
            out.push_str("- ");
            out.push_str(&line[column + 2..]);
            spliced = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
}

/// Comments out a rendered entry, putting the `#` after each line's
/// indentation so deleting it restores the entry in place.
fn comment_out(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines() {
        let indent = line.len() - line.trim_start().len();
        out.push_str(&line[..indent]);
        out.push('#');
        out.push_str(&line[indent..]);
        out.push('\n');
    }
    out
}

/// Writes one scalar. A non-finite float has a YAML literal, so it writes
/// rather than failing the way it does in JSON and HCL.
fn write_scalar(out: &mut String, scalar: &Scalar) {
    match scalar {
        // An unparsed literal reached the model as text from an environment
        // variable or a flag, so it writes as the string it always was.
        Scalar::String(text) | Scalar::Unparsed(text) => write_string(out, text),
        Scalar::Int(int) => out.push_str(&int.to_string()),
        Scalar::Bool(boolean) => out.push_str(if *boolean { "true" } else { "false" }),
        Scalar::Float(float) => out.push_str(&float_text(*float)),
    }
}

/// A float's text, in a form the core schema reads back as a float.
///
/// The `Debug` formatting of a finite `f64` always writes a fraction or an
/// exponent, so the resolution never reads it as an integer. YAML 1.2 spells
/// the three non-finite values natively.
fn float_text(float: f64) -> String {
    if float.is_nan() {
        return ".nan".to_string();
    }
    if float.is_infinite() {
        return if float.is_sign_negative() { "-.inf" } else { ".inf" }.to_string();
    }
    format!("{float:?}")
}

/// Writes a key bare when it is an ASCII identifier, and double-quoted
/// otherwise.
fn write_key(out: &mut String, name: &str) {
    if plain_key(name) {
        out.push_str(name);
    } else {
        write_string(out, name);
    }
}

/// Whether a key is plainly safe: ASCII letters, digits, `_`, and `-`, opening
/// with a letter or `_`. The check is deliberately narrow, because a key that
/// resolves to something other than a string would change meaning.
fn plain_key(name: &str) -> bool {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    characters.all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

/// Writes a double-quoted scalar, escaping the quote, the backslash, and every
/// control character, with the short escapes where they exist. Everything else
/// writes as raw UTF-8, which YAML permits, so non-ASCII text stays readable.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if control < '\u{20}' => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// Writes one level of indentation for each nesting depth.
fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

