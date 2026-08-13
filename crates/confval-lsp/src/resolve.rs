//! Position resolution: a byte offset to a [`CursorContext`].
//!
//! The block-structured formats converge on `confval`'s neutral [`Fields`] tree,
//! which already carries the name span, value span, and block span of every
//! field. So the walk is shared: it descends the tree following the block whose
//! span contains the offset, and it reads the position kind from the field the
//! offset lands on. When no tree is available, or the offset lands between
//! fields, it scans the raw text for the identifier under the cursor.

use confval::format::{FieldKind, Fields, Value, ValueKind};
use confval::source::Span;

use crate::frontend::CursorContext;

/// Resolves `offset` against the retained tree, falling back to a text scan.
pub(crate) fn resolve_in_tree(tree: Option<&Fields>, text: &str, offset: usize) -> CursorContext {
    let mut path = Vec::new();
    let mut level = match tree {
        Some(level) => level,
        None => return CursorContext::body(path, scan_identifier(text, offset)),
    };

    loop {
        match descend(level, text, offset) {
            Step::Enter(name, inner) => {
                path.push(name);
                level = inner;
            }
            Step::Here(mut context) => {
                context.path = {
                    let mut full = path;
                    full.append(&mut context.path);
                    full
                };
                return context;
            }
        }
    }
}

/// One decision at a level: descend into a block, or resolve here.
enum Step<'a> {
    /// The offset is inside this block's body. Descend, recording the name.
    Enter(String, &'a Fields),
    /// The offset resolves at this level. The context's `path` is empty and the
    /// caller prepends the accumulated path.
    Here(CursorContext),
}

/// Classifies `offset` against one level's fields.
fn descend<'a>(level: &'a Fields, text: &str, offset: usize) -> Step<'a> {
    for field in level.iter() {
        match &field.kind {
            FieldKind::Block(inner) => {
                if inside_body(field.name_span, block_body_end(field.span, inner), offset) {
                    return Step::Enter(field.name.clone(), inner);
                }
                if contains(field.name_span, offset) {
                    return Step::Here(CursorContext::body(Vec::new(), token_of(field.name_span)));
                }
            }
            FieldKind::Value(value) => match &value.kind {
                ValueKind::Map(inner) => {
                    if contains(value.span, offset) {
                        return Step::Enter(field.name.clone(), inner);
                    }
                    if contains(field.name_span, offset) {
                        return Step::Here(CursorContext::body(
                            Vec::new(),
                            token_of(field.name_span),
                        ));
                    }
                }
                _ => {
                    if contains(value.span, offset) {
                        return Step::Here(CursorContext::attribute_value(
                            Vec::new(),
                            field.name.clone(),
                            token_of(value.span),
                        ));
                    }
                    if contains(field.name_span, offset) {
                        return Step::Here(CursorContext::body(
                            Vec::new(),
                            token_of(field.name_span),
                        ));
                    }
                }
            },
        }
    }
    Step::Here(CursorContext::body(
        Vec::new(),
        scan_identifier(text, offset),
    ))
}

/// Whether `offset` sits inside a block's body, past its name and within its
/// body extent.
fn inside_body(name_span: Span, body_end: u32, offset: usize) -> bool {
    if name_span.is_detached() {
        return false;
    }
    (name_span.end as usize) < offset && offset <= (body_end as usize)
}

/// A block's body extent: the furthest end among the block's own span and its
/// descendants. A TOML `[table]` span covers only the header, so the block's
/// entries extend the body past it. HCL and KDL block spans already cover their
/// entries, so the furthest end leaves them unchanged.
fn block_body_end(block_span: Span, inner: &Fields) -> u32 {
    end_of(block_span).max(deepest_end(inner))
}

/// The furthest non-detached end offset among a level's fields and their
/// descendants.
fn deepest_end(fields: &Fields) -> u32 {
    let mut furthest = 0;
    for field in fields.iter() {
        furthest = furthest.max(end_of(field.span));
        match &field.kind {
            FieldKind::Block(inner) => furthest = furthest.max(deepest_end(inner)),
            FieldKind::Value(value) => furthest = furthest.max(deepest_end_value(value)),
        }
    }
    furthest
}

/// The furthest non-detached end offset within a value, recursing through maps
/// and sequences.
fn deepest_end_value(value: &Value) -> u32 {
    let mut furthest = end_of(value.span);
    match &value.kind {
        ValueKind::Map(inner) => furthest = furthest.max(deepest_end(inner)),
        ValueKind::Seq(items) => {
            for item in items {
                furthest = furthest.max(deepest_end_value(item));
            }
        }
        _ => {}
    }
    furthest
}

/// The end offset of a span, or zero for the detached sentinel.
fn end_of(span: Span) -> u32 {
    if span.is_detached() { 0 } else { span.end }
}

/// Whether `offset` falls within `span`, inclusive of the end so a cursor just
/// past the last character still resolves.
fn contains(span: Span, offset: usize) -> bool {
    !span.is_detached() && (span.start as usize) <= offset && offset <= (span.end as usize)
}

/// The byte range of a span, for a completion replace range.
fn token_of(span: Span) -> Option<(usize, usize)> {
    (!span.is_detached()).then_some((span.start as usize, span.end as usize))
}

/// The identifier the cursor sits in or at the end of, scanned from raw text.
/// Returns `None` when no identifier character is adjacent.
fn scan_identifier(text: &str, offset: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let offset = offset.min(bytes.len());
    let mut start = offset;
    while start > 0 && is_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    (start != end).then_some((start, end))
}

/// Whether a byte is part of a config field identifier.
fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}
