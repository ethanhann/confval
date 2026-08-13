//! Structural recovery from raw text, for a buffer that does not parse.
//!
//! When the current buffer parses, resolution walks the neutral field tree,
//! whose spans align with the text exactly. When it does not, this module
//! reconstructs the enclosing block path and the position kind from the raw
//! text, so completion still resolves inside the block the cursor sits in and at
//! the value the cursor sits on. It reads only the current text, so its offsets
//! are always current.

use crate::frontend::CursorContext;
use crate::resolve::{identifier_token, value_token};

/// Resolves an offset from raw text.
///
/// `braces` is true for a brace-delimited format (HCL, KDL) and false for a
/// header format (TOML). `equals` is true when an attribute separates its name
/// and value with `=` (HCL, TOML) and false for a whitespace separator (KDL).
/// `hash_comment` is true when `#` starts a line comment (HCL) and false when it
/// does not (KDL spells booleans `#true`).
pub(crate) fn resolve_in_text(
    text: &str,
    offset: usize,
    braces: bool,
    equals: bool,
    hash_comment: bool,
) -> CursorContext {
    let offset = floor_char_boundary(text, offset);
    let path = if braces {
        brace_path(text, offset, hash_comment)
    } else {
        header_path(text, offset)
    };
    match attribute_name(text, offset, equals) {
        Some(field) => CursorContext::attribute_value(path, field, value_token(text, offset)),
        None => CursorContext::body(path, identifier_token(text, offset)),
    }
}

/// The enclosing block path in a brace-delimited format: the identifiers of the
/// blocks whose braces are open at the offset.
fn brace_path(text: &str, offset: usize, hash_comment: bool) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<String> = Vec::new();
    let mut index = 0;
    while index < offset {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index),
            b'#' if hash_comment => index = skip_line(bytes, index),
            b'/' if bytes.get(index + 1) == Some(&b'/') => index = skip_line(bytes, index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => index = skip_block_comment(bytes, index),
            b'{' => {
                stack.push(statement_identifier(text, index));
                index += 1;
            }
            b'}' => {
                stack.pop();
                index += 1;
            }
            _ => index += 1,
        }
    }
    stack.into_iter().filter(|name| !name.is_empty()).collect()
}

/// The leading identifier of the statement that opens a block, so `server {` and
/// `server "label" {` and `server = {` all name `server`.
fn statement_identifier(text: &str, brace: usize) -> String {
    let bytes = text.as_bytes();
    let mut start = brace;
    while start > 0 && !matches!(bytes[start - 1], b'\n' | b'{' | b'}') {
        start -= 1;
    }
    first_identifier(&text[start..brace])
}

/// The enclosing table path in a header format: the segments of the last table
/// header at or before the offset.
fn header_path(text: &str, offset: usize) -> Vec<String> {
    let mut path = Vec::new();
    for line in text[..offset].split('\n') {
        if let Some(header) = parse_header(line) {
            path = header;
        }
    }
    path
}

/// The dotted segments of a TOML `[table]` or `[[array]]` header, or `None`.
fn parse_header(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix("[[")
        .and_then(|rest| rest.strip_suffix("]]"))
        .or_else(|| {
            trimmed
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
        })?;
    Some(
        inner
            .split('.')
            .map(|segment| segment.trim().trim_matches('"').to_string())
            .filter(|segment| !segment.is_empty())
            .collect(),
    )
}

/// The name of the attribute whose value the cursor sits in, or `None` for a body
/// position.
fn attribute_name(text: &str, offset: usize, equals: bool) -> Option<String> {
    let line_start = text[..offset]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line = &text[line_start..offset];
    if equals {
        attribute_name_equals(line)
    } else {
        attribute_name_space(line)
    }
}

/// A value position in an `=` format: the line has a top-level `=` before the
/// cursor with an identifier before it.
fn attribute_name_equals(line: &str) -> Option<String> {
    let equals = top_level_equals(line)?;
    last_identifier(&line[..equals])
}

/// A value position in a whitespace format: the line names a node and the cursor
/// sits past the node name in its argument region, with no block brace.
fn attribute_name_space(line: &str) -> Option<String> {
    if line.contains('{') {
        return None;
    }
    let indent: usize = line.len() - line.trim_start().len();
    let node = &line[indent..];
    let name_len = node.bytes().take_while(|byte| is_identifier(*byte)).count();
    if name_len == 0 || name_len >= node.len() {
        return None;
    }
    node.as_bytes()[name_len]
        .is_ascii_whitespace()
        .then(|| node[..name_len].to_string())
}

/// The offset of the first `=` on a line that is not inside a string.
fn top_level_equals(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index),
            b'=' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

/// The first identifier run in a segment, or the empty string.
fn first_identifier(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let start = bytes.iter().position(|byte| is_identifier(*byte));
    match start {
        Some(start) => {
            let end = start
                + bytes[start..]
                    .iter()
                    .take_while(|byte| is_identifier(**byte))
                    .count();
            segment[start..end].to_string()
        }
        None => String::new(),
    }
}

/// The last identifier run in a segment, or `None`.
fn last_identifier(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut end = bytes.len();
    while end > 0 && !is_identifier(bytes[end - 1]) {
        end -= 1;
    }
    if end == 0 {
        return None;
    }
    let mut start = end;
    while start > 0 && is_identifier(bytes[start - 1]) {
        start -= 1;
    }
    Some(segment[start..end].to_string())
}

/// The index just past a string literal that starts at `open`, honoring `\"`.
fn skip_string(bytes: &[u8], open: usize) -> usize {
    let mut index = open + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    index
}

/// The index just past a `/* */` block comment that starts at `start`.
fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 2;
    while index + 1 < bytes.len() {
        if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            return index + 2;
        }
        index += 1;
    }
    bytes.len()
}

/// The index just past the newline that ends the line containing `start`.
fn skip_line(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    while index < bytes.len() && bytes[index] != b'\n' {
        index += 1;
    }
    if index < bytes.len() {
        index + 1
    } else {
        index
    }
}

/// Whether a byte is part of an identifier.
fn is_identifier(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// The largest char boundary at or before `offset`, clamped to the text length.
fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::PositionKind;

    fn value(field: &str) -> PositionKind {
        PositionKind::AttributeValue {
            field: field.to_string(),
        }
    }

    #[test]
    fn hcl_empty_value_resolves_to_the_attribute_value_in_the_block() {
        // Arrange
        // `mode = ` with an empty value does not parse. The text scan must place
        // the cursor at `mode`'s value inside `limits`.
        let text = "limits {\n  max_body_mb = 10\n  mode = \n}\n";
        let offset = text.find("mode = ").unwrap() + "mode = ".len();

        // Act
        let context = resolve_in_text(text, offset, true, true, true);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, value("mode"));
    }

    #[test]
    fn toml_empty_value_resolves_to_the_attribute_value_in_the_table() {
        // Arrange
        let text = "[limits]\nmax_body_mb = 10\nmode = \n";
        let offset = text.find("mode = ").unwrap() + "mode = ".len();

        // Act
        let context = resolve_in_text(text, offset, false, true, true);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, value("mode"));
    }

    #[test]
    fn kdl_empty_value_resolves_to_the_node_argument_in_the_block() {
        // Arrange
        let text = "limits {\n  max_body_mb 10\n  mode \n}\n";
        let offset = text.find("mode ").unwrap() + "mode ".len();

        // Act
        let context = resolve_in_text(text, offset, true, false, false);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, value("mode"));
    }

    #[test]
    fn a_half_typed_name_in_a_block_resolves_to_a_body_position() {
        // Arrange
        let text = "limits {\n  max_body_mb = 10\n  mo\n}\n";
        let offset = text.find("  mo\n").unwrap() + "  mo".len();

        // Act
        let context = resolve_in_text(text, offset, true, true, true);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
        let (start, end) = context.token;
        assert_eq!(&text[start..end], "mo");
    }

    #[test]
    fn a_brace_inside_a_string_does_not_open_a_block() {
        // Arrange
        let text = "headers = { \"a\" = \"{\" }\nport = \n";
        let offset = text.find("port = ").unwrap() + "port = ".len();

        // Act
        let context = resolve_in_text(text, offset, true, true, true);

        // Assert
        // The `{` in the string must not leave a block open.
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, value("port"));
    }

    #[test]
    fn a_brace_inside_a_block_comment_does_not_close_a_block() {
        // Arrange
        // The `}` sits inside an HCL `/* */` comment, so it must not close the
        // `server` block.
        let text = "server {\n  /* } */\n  port = \n}\n";
        let offset = text.find("port = ").unwrap() + "port = ".len();

        // Act
        let context = resolve_in_text(text, offset, true, true, true);

        // Assert
        assert_eq!(context.path, vec!["server".to_string()]);
        assert_eq!(context.kind, value("port"));
    }

    #[test]
    fn a_kdl_hash_keyword_is_not_a_comment() {
        // Arrange
        // KDL spells booleans `#true`, not a comment, so the following brace must
        // still open the `server` block.
        let text = "server #true {\n  port \n}\n";
        let offset = text.find("port ").unwrap() + "port ".len();

        // Act
        let context = resolve_in_text(text, offset, true, false, false);

        // Assert
        assert_eq!(context.path, vec!["server".to_string()]);
        assert_eq!(context.kind, value("port"));
    }
}
