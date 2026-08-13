//! Position resolution over a parsed tree: a byte offset to a [`CursorContext`].
//!
//! HCL, TOML, KDL, and JSON converge on `confval`'s neutral [`Fields`] tree,
//! which already carries the name span, value span, and block span of every
//! field. The walk is therefore shared. It descends the tree following the block
//! whose span contains the offset and reads the position kind from the field the
//! offset lands on. When the offset lands between fields, it scans the raw text
//! for the identifier under the cursor. Recovery for a buffer that does not parse
//! is in [`text`](crate::scan::text), and YAML resolves from indentation in
//! [`yaml`](crate::scan::yaml).

use confval::format::{Field, FieldKind, Fields, Value, ValueKind};
use confval::source::Span;

use crate::frontend::CursorContext;

/// Resolves `offset` against the parsed tree.
pub(crate) fn resolve_in_tree(
    tree: &Fields,
    text: &str,
    offset: usize,
    covers_body: bool,
) -> CursorContext {
    let mut path = Vec::new();
    let mut level = tree;

    loop {
        match descend(level, text, offset, covers_body) {
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
fn descend<'a>(level: &'a Fields, text: &str, offset: usize, covers_body: bool) -> Step<'a> {
    let fields: Vec<&Field> = level.iter().collect();
    let enclosing_end = end_of(level.enclosing());
    for (index, &field) in fields.iter().enumerate() {
        let next = fields.get(index + 1).map(|sibling| start_of(sibling.span));
        match &field.kind {
            FieldKind::Block(inner) => {
                if in_block_body(field, inner, covers_body, next, enclosing_end, offset) {
                    return Step::Enter(field.name.clone(), inner);
                }
            }
            FieldKind::Value(value) => match &value.kind {
                ValueKind::Map(inner) => {
                    if contains(value.span, offset) {
                        return Step::Enter(field.name.clone(), inner);
                    }
                }
                ValueKind::Seq(elements) => {
                    if let Some(inner) = seq_element_body(elements, next, enclosing_end, offset) {
                        return Step::Enter(field.name.clone(), inner);
                    }
                    if contains(value.span, offset) {
                        return Step::Here(CursorContext::attribute_value(
                            Vec::new(),
                            field.name.clone(),
                            value_replace_token(field, value, text, offset),
                        ));
                    }
                }
                _ => {
                    if contains(value.span, offset) {
                        return Step::Here(CursorContext::attribute_value(
                            Vec::new(),
                            field.name.clone(),
                            value_replace_token(field, value, text, offset),
                        ));
                    }
                }
            },
        }
        if contains(field.name_span, offset) {
            return Step::Here(CursorContext::body(
                Vec::new(),
                identifier_token(text, offset),
            ));
        }
    }
    Step::Here(CursorContext::body(
        Vec::new(),
        identifier_token(text, offset),
    ))
}

/// The body of the array-of-tables element that contains `offset`, if any.
///
/// Each element is a header-only block, so its body runs to the next element, or
/// to the array field's next sibling or the enclosing end for the last element.
fn seq_element_body(
    elements: &[Value],
    next_sibling_start: Option<u32>,
    enclosing_end: u32,
    offset: usize,
) -> Option<&Fields> {
    for (index, element) in elements.iter().enumerate() {
        let ValueKind::Map(inner) = &element.kind else {
            continue;
        };
        let start = start_of(element.span) as usize;
        let within = match elements.get(index + 1) {
            Some(following) => offset < start_of(following.span) as usize,
            None => {
                offset
                    <= next_sibling_start
                        .unwrap_or(enclosing_end)
                        .max(deepest_end(inner)) as usize
            }
        };
        if start <= offset && within {
            return Some(inner);
        }
    }
    None
}

/// The completion replace range for an attribute value.
///
/// A value that begins after the node name is a real value, replaced whole. A
/// KDL node with no argument parses with its value span on the node name, so
/// there is no value to replace. It inserts at the cursor instead, clamped to
/// stay past the name, so completing the value never overwrites the name.
fn value_replace_token(field: &Field, value: &Value, text: &str, offset: usize) -> (usize, usize) {
    let has_value = !field.name_span.is_detached()
        && !value.span.is_detached()
        && value.span.start > field.name_span.end;
    if has_value {
        return span_token(value.span, text);
    }
    let name_end = field.name_span.end as usize;
    let (start, end) = value_token(text, offset);
    (start.max(name_end), end.max(name_end))
}

/// The completion replace token for the value of `name` at `path` in the parsed
/// tree, or `None` when the field or its value is absent. YAML resolution reads
/// its path and kind from indentation, but takes the value token from the tree
/// when the buffer parses, so a completion replaces the whole value rather than
/// stopping at a space.
pub(crate) fn value_span_token(
    tree: &Fields,
    path: &[String],
    name: &str,
    text: &str,
) -> Option<(usize, usize)> {
    let level = crate::walk::fields_at(tree, path)?;
    let field = level.get(name)?;
    match &field.kind {
        FieldKind::Value(value) if !value.span.is_detached() => Some(span_token(value.span, text)),
        _ => None,
    }
}

/// The completion replace range for a parsed value: its exact span, clamped to
/// the cursor's line so a value with a space is replaced whole and a multi-line
/// value does not overreach.
fn span_token(span: Span, text: &str) -> (usize, usize) {
    if span.is_detached() {
        return (text.len(), text.len());
    }
    let start = (span.start as usize).min(text.len());
    let mut end = (span.end as usize).min(text.len());
    if let Some(newline) = text.get(start..).and_then(|rest| rest.find('\n')) {
        end = end.min(start + newline);
    }
    (start, end)
}

/// Whether `offset` sits inside a block's body, past its name.
///
/// A block whose span covers its body (HCL, KDL) is bounded by that span. A
/// header-only block (a TOML table) owns the region up to the next sibling, or
/// to the enclosing level's end when it is the last field.
fn in_block_body(
    field: &Field,
    inner: &Fields,
    covers_body: bool,
    next_sibling_start: Option<u32>,
    enclosing_end: u32,
    offset: usize,
) -> bool {
    if field.name_span.is_detached() || offset <= field.name_span.end as usize {
        return false;
    }
    if covers_body {
        return offset <= block_body_end(field.span, inner) as usize;
    }
    match next_sibling_start {
        Some(start) => offset < start as usize,
        // The last header-only block extends to the enclosing end, or past it to
        // its own furthest child, because a nested table's enclosing span is only
        // the parent header.
        None => offset <= enclosing_end.max(deepest_end(inner)) as usize,
    }
}

/// The start offset of a span, or zero for the detached sentinel.
fn start_of(span: Span) -> u32 {
    if span.is_detached() { 0 } else { span.start }
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

/// The completion replace range for a body position: the identifier the cursor
/// sits in or at the end of, scanned from the current text, or a zero-width range
/// at the cursor when no identifier is adjacent. Reading the current text rather
/// than the parse keeps the range valid and on the cursor's line even when the
/// buffer does not parse.
pub(crate) fn identifier_token(text: &str, offset: usize) -> (usize, usize) {
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
    (start, end)
}

/// The completion replace range for a value position: the run of value
/// characters the cursor sits in, scanned from the current text and bounded to
/// the cursor's line, so replacing an enum value never reaches across a line.
pub(crate) fn value_token(text: &str, offset: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let offset = offset.min(bytes.len());
    let mut start = offset;
    while start > 0 && is_value_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_value_byte(bytes[end]) {
        end += 1;
    }
    (start, end)
}

/// Whether a byte is part of a config field identifier.
fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Whether a byte is part of a scalar value token. Whitespace and the structural
/// delimiters bound the token, so it stays within one value on one line.
fn is_value_byte(byte: u8) -> bool {
    !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'{' | b'}' | b'[' | b']' | b',')
}
