//! The member model of the YAML write path: what one level's entries group
//! into, and how each group renders.
//!
//! A level's fields become members, one per name, values before blocks. Several
//! same-named fields become one member holding a sequence, because YAML has no
//! second way to write a repeated name.
//!
//! Each member then has a shape, which decides its layout. The shape turns on
//! how the member renders rather than on its field kind. A kind-keyed rule
//! would make emit non-idempotent, because a parse of the emitted text yields
//! map values where the original held blocks.

use crate::format::emit::{grouped_elements, values_then_blocks};
use crate::format::field::{Field, FieldKind, Fields, Value, ValueKind};

/// One member of an emitted mapping: the same-named fields sharing its name.
pub(super) enum Member<'a> {
    Values(Vec<&'a Value>),
    Blocks(Vec<&'a Fields>),
}

/// How a member renders, which decides its layout.
///
/// The distinction turns on the rendering rather than on the field's kind. A
/// kind-keyed rule would make emit non-idempotent, because a parse of the
/// emitted text yields `Map` values where the original held blocks.
#[derive(PartialEq, Clone, Copy)]
pub(super) enum Shape {
    /// The whole member fits after `key: `.
    Inline,
    /// The member's body renders on the lines below `key:`.
    Block,
}

/// One member ready to render, with the annotation a template has.
pub(super) struct Rendered<'a> {
    pub(super) name: &'a str,
    pub(super) doc: Option<&'a str>,
    pub(super) commented: bool,
    pub(super) member: Member<'a>,
}

/// The members of one level, values before blocks, each group at its first
/// occurrence's position. A commented entry stands alone, so it never joins a
/// group and never blocks an active field.
pub(super) fn members_of(fields: &Fields) -> Vec<Rendered<'_>> {
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
            // takes the first doc any of its fields has.
            doc: group.iter().find_map(|other| other.doc.as_deref()),
            commented: false,
            member,
        });
    }
    members
}

/// The member one field forms on its own.
pub(super) fn lone(field: &Field) -> Member<'_> {
    match &field.kind {
        FieldKind::Value(value) => Member::Values(vec![value]),
        FieldKind::Block(inner) => Member::Blocks(vec![inner]),
    }
}

/// How a member renders. A lone scalar, an empty collection, and a sequence of
/// scalars all fit on the key's line.
///
/// A block with no active field renders inline as `{}` even when it holds
/// commented entries, because comment lines below `key:` alone read back as
/// null rather than as an empty mapping.
pub(super) fn shape_of(member: &Member) -> Shape {
    match member {
        Member::Values(group) => match group.as_slice() {
            [only] => shape_of_value(only),
            _ => shape_of_elements(&grouped_elements(group)),
        },
        Member::Blocks(group) => match group.as_slice() {
            [only] if only.iter().next().is_none() => Shape::Inline,
            _ => Shape::Block,
        },
    }
}

pub(super) fn shape_of_value(value: &Value) -> Shape {
    match &value.kind {
        ValueKind::Seq(elements) => shape_of_elements(&elements.iter().collect::<Vec<_>>()),
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
pub(super) fn shape_of_elements(elements: &[&Value]) -> Shape {
    let structural = elements
        .iter()
        .any(|element| matches!(element.kind, ValueKind::Map(_) | ValueKind::Seq(_)));
    if structural {
        Shape::Block
    } else {
        Shape::Inline
    }
}
