//! Structural recovery from raw text, for a buffer that does not parse.
//!
//! When the current buffer parses, resolution walks the neutral field tree,
//! whose spans align with the text exactly. When it does not, this module
//! reconstructs the enclosing block path and the position kind from the raw
//! text, so completion still resolves inside the block the cursor sits in and at
//! the value the cursor sits on. It reads only the current text, so its offsets
//! are always current.

use super::json::object_path;
use crate::encoding::floor_char_boundary;
use crate::frontend::{CursorContext, ValueSeparator};
use crate::resolve::{identifier_token, value_token};

/// The reconstruction the raw-text scan runs, one variant per reader it has.
/// An indentation format has no variant here, so the scan cannot be asked to
/// recover a format its readers do not cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextRecovery {
    /// A brace-delimited block language: HCL, KDL.
    Braces,
    /// A header-addressed table language: TOML.
    Header,
    /// A brace-delimited object language with quoted keys: JSON.
    Object,
}

/// Resolves an offset from raw text.
///
/// `recovery` selects how the enclosing path is reconstructed, and `separator`
/// selects how a value position is detected. `comments` is the format's
/// line-comment vocabulary, read to skip a comment while scanning blocks and
/// to refuse a value position inside one.
pub(crate) fn resolve_in_text(
    text: &str,
    offset: usize,
    recovery: TextRecovery,
    separator: ValueSeparator,
    comments: &[&str],
) -> CursorContext {
    let offset = floor_char_boundary(text, offset);
    let path = match recovery {
        TextRecovery::Braces => brace_path(text, offset, comments),
        TextRecovery::Header => header_path(text, offset),
        TextRecovery::Object => object_path(text, offset),
    };
    match attribute_name(text, offset, separator, comments) {
        Some((field, value_start)) => {
            // In a colon format the scanned value token walks back through the
            // colon and the quoted key, because both are value bytes. The clamp
            // keeps the token past the colon, so a completion never replaces
            // the key. The other separators bound the token themselves.
            let (start, end) = value_token(text, offset);
            let token = match value_start {
                Some(after) => (start.max(after), end.max(after)),
                None => (start, end),
            };
            CursorContext::attribute_value(path, field, token)
        }
        None => CursorContext::body(path, identifier_token(text, offset)),
    }
}

/// The enclosing block path in a brace-delimited format: the identifiers of the
/// blocks whose braces are open at the offset.
fn brace_path(text: &str, offset: usize, comments: &[&str]) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<String> = Vec::new();
    let mut index = 0;
    while index < offset {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index),
            _ if starts_comment(&bytes[index..], comments) => index = skip_line(bytes, index),
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

/// The name of the attribute whose value the cursor sits in, with the byte
/// offset just past its separator when the separator is a value byte, or `None`
/// for a body position. A cursor past a comment marker sits in the comment,
/// which is no value position.
fn attribute_name(
    text: &str,
    offset: usize,
    separator: ValueSeparator,
    comments: &[&str],
) -> Option<(String, Option<usize>)> {
    let line_start = text[..offset]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line = &text[line_start..offset];
    if past_comment(line, comments) {
        return None;
    }
    match separator {
        ValueSeparator::Equals => attribute_name_equals(line).map(|name| (name, None)),
        ValueSeparator::Colon => {
            attribute_name_colon(line).map(|(name, colon)| (name, Some(line_start + colon + 1)))
        }
        ValueSeparator::Whitespace => attribute_name_space(line).map(|name| (name, None)),
    }
}

/// A value position in a `:` format (JSON): the member key whose value the cursor
/// sits in and the key's colon offset within the line, or `None` for a body
/// position. It resets at each object or array bracket and comma, so a `"key":`
/// in an enclosing or a sibling member does not classify a cursor that sits in a
/// fresh element as a value.
fn attribute_name_colon(line: &str) -> Option<(String, usize)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut candidate: Option<String> = None;
    let mut pending: Option<(String, usize)> = None;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                let start = index;
                index = skip_string(bytes, start);
                let content_end = if index > start + 1 && bytes.get(index - 1) == Some(&b'"') {
                    index - 1
                } else {
                    index
                };
                candidate = Some(line[start + 1..content_end].to_string());
            }
            b'{' | b'[' | b'}' | b']' | b',' => {
                candidate = None;
                pending = None;
                index += 1;
            }
            b':' => {
                pending = candidate.take().map(|name| (name, index));
                index += 1;
            }
            _ => index += 1,
        }
    }
    pending
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

/// Whether the segment starts with one of the format's comment markers. The
/// segment is bytes rather than text because the scanners index byte by byte,
/// and a byte index may sit inside a multi-byte character.
fn starts_comment(segment: &[u8], comments: &[&str]) -> bool {
    comments
        .iter()
        .any(|marker| segment.starts_with(marker.as_bytes()))
}

/// Whether the cursor's line prefix holds a comment start outside a string, so
/// the cursor sits inside the comment.
fn past_comment(line: &str, comments: &[&str]) -> bool {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            index = skip_string(bytes, index);
            continue;
        }
        if starts_comment(&bytes[index..], comments) {
            return true;
        }
        index += 1;
    }
    false
}

/// The index just past a string literal that starts at `open`, honoring `\"`.
/// An unterminated string ends at the buffer, even when its last byte is a
/// backslash whose escape would step past the end.
pub(crate) fn skip_string(bytes: &[u8], open: usize) -> usize {
    let mut index = open + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
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
pub(crate) fn is_identifier(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
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
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Equals,
            &["#", "//"],
        );

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
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Header,
            ValueSeparator::Equals,
            &["#"],
        );

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
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Whitespace,
            &["//"],
        );

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
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Equals,
            &["#", "//"],
        );

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
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Equals,
            &["#", "//"],
        );

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
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Equals,
            &["#", "//"],
        );

        // Assert
        assert_eq!(context.path, vec!["server".to_string()]);
        assert_eq!(context.kind, value("port"));
    }

    #[test]
    fn a_json_value_token_starts_past_the_member_colon() {
        // Arrange
        // The buffer does not parse, and the value bytes run back through the
        // colon and the quoted key. The token must start past the colon, so a
        // completion never replaces the key.
        let text = "{\n  \"mode\":en\n}\n";
        let offset = text.find(":en").unwrap() + ":en".len();

        // Act
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Object,
            ValueSeparator::Colon,
            &[],
        );

        // Assert
        assert_eq!(context.kind, value("mode"));
        let (start, end) = context.token;
        assert_eq!(&text[start..end], "en", "the value alone, not the key");
    }

    #[test]
    fn a_multibyte_character_in_an_hcl_value_does_not_panic_the_brace_scan() {
        // Arrange
        // The brace scan crosses `é`, whose continuation byte is not a char
        // boundary. Slicing the text there panicked before the byte-wise fix.
        let text = "region = eu-wést-1\n";
        let offset = text.len() - 1;

        // Act
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Equals,
            &["#", "//"],
        );

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, value("region"));
    }

    #[test]
    fn a_multibyte_character_in_a_toml_value_does_not_panic_the_comment_scan() {
        // Arrange
        let text = "[a]\nname = héllo";
        let offset = text.len();

        // Act
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Header,
            ValueSeparator::Equals,
            &["#"],
        );

        // Assert
        assert_eq!(context.path, vec!["a".to_string()]);
        assert_eq!(context.kind, value("name"));
    }

    #[test]
    fn a_multibyte_character_in_a_kdl_node_name_does_not_panic_the_scan() {
        // Arrange
        let text = "café 1\n";
        let offset = text.len() - 1;

        // Act
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Whitespace,
            &["//"],
        );

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_line_ending_in_a_backslash_does_not_panic_the_colon_scan() {
        // Arrange
        // The trailing backslash makes `skip_string` step past the end of the
        // line, and the key slice panicked before the clamp.
        let text = "{\n  \"path\": \"C:\\";
        let offset = text.len();

        // Act
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Object,
            ValueSeparator::Colon,
            &[],
        );

        // Assert
        assert_eq!(context.kind, value("path"));
    }

    #[test]
    fn a_kdl_hash_keyword_is_not_a_comment() {
        // Arrange
        // KDL writes booleans `#true`, not a comment, so the following brace must
        // still open the `server` block.
        let text = "server #true {\n  port \n}\n";
        let offset = text.find("port ").unwrap() + "port ".len();

        // Act
        let context = resolve_in_text(
            text,
            offset,
            TextRecovery::Braces,
            ValueSeparator::Whitespace,
            &["//"],
        );

        // Assert
        assert_eq!(context.path, vec!["server".to_string()]);
        assert_eq!(context.kind, value("port"));
    }
}
