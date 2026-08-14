//! Indentation-based resolution for YAML, in both parse states.
//!
//! Block YAML nests by indentation rather than a delimiter, and a key that awaits
//! its nested body parses as null with a span that stops at the key line. So the
//! tree does not cover a pending body position, and resolution reconstructs the
//! enclosing path and the position kind from indentation and the current line,
//! whether or not the buffer parses.

use crate::encoding::floor_char_boundary;
use crate::frontend::CursorContext;
use crate::resolve::{identifier_token, value_token};

/// Resolves a byte offset in a YAML document from its indentation.
pub(crate) fn resolve_in_yaml(text: &str, offset: usize) -> CursorContext {
    let offset = floor_char_boundary(text, offset);
    let path = yaml_path(text, offset);
    match yaml_attribute(text, offset) {
        Some(field) => CursorContext::attribute_value(path, field, value_token(text, offset)),
        None => CursorContext::body(path, identifier_token(text, offset)),
    }
}

/// The enclosing key path at the offset, read from indentation.
///
/// It walks the lines above the cursor top down, keeping a stack of the open
/// `key:` frames by their column, then keeps the frames indented less than the
/// cursor, which are the cursor's ancestors. A block scalar's body is skipped, so
/// its indented content is not read as structure.
fn yaml_path(text: &str, offset: usize) -> Vec<String> {
    let line_start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let cursor_slice = &text[line_start..offset];
    let cursor_line = cursor_slice.strip_suffix('\r').unwrap_or(cursor_slice);
    let cursor_indent = line_indent(cursor_line);

    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut block_scalar: Option<usize> = None;
    let mut flow_depth: i32 = 0;
    for raw in text[..line_start].split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        // A multi-line flow collection nests inline, not by indentation, so its
        // interior lines are skipped until the flow closes.
        if flow_depth > 0 {
            flow_depth += flow_delta(line);
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if let Some(header_indent) = block_scalar {
            if indent > header_indent {
                continue;
            }
            block_scalar = None;
        }
        if let Some((col, name, opens_scalar)) = parse_key(line, indent) {
            while let Some((last, _)) = stack.last() {
                if *last >= col {
                    stack.pop();
                } else {
                    break;
                }
            }
            if opens_scalar {
                block_scalar = Some(indent);
            }
            stack.push((col, name));
        }
        flow_depth += flow_delta(line);
        if flow_depth < 0 {
            flow_depth = 0;
        }
    }
    stack
        .into_iter()
        .filter(|(col, _)| *col < cursor_indent)
        .map(|(_, name)| name)
        .collect()
}

/// The net flow-bracket balance of a line, counting `{` and `[` as open and `}`
/// and `]` as close, outside a quoted string or a comment. A positive running
/// total means a flow collection is open across lines.
fn flow_delta(line: &str) -> i32 {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut delta = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_double(bytes, index),
            b'\'' => index = skip_single(bytes, index),
            b'#' => break,
            b'{' | b'[' => {
                delta += 1;
                index += 1;
            }
            b'}' | b']' => {
                delta -= 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    delta
}

/// The attribute whose value the cursor sits in, or `None` for a body position.
///
/// A line with a `key:` followed by content, a space, or the end of the line is
/// an attribute-value position. A fresh line with no `key:` is a body position,
/// which is where a value on the next line leaves the cursor.
fn yaml_attribute(text: &str, offset: usize) -> Option<String> {
    let line_start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let slice = &text[line_start..offset];
    let prefix = slice.strip_suffix('\r').unwrap_or(slice);
    let trimmed = prefix.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .map(str::trim_start)
        .unwrap_or(trimmed);
    let colon = find_top_colon(rest)?;
    let after = &rest[colon + 1..];
    if !(after.is_empty() || after.starts_with(' ')) {
        return None;
    }
    let name = rest[..colon].trim().trim_matches('"');
    (!name.is_empty()).then(|| name.to_string())
}

/// The indentation column of a line, counting a `- ` sequence marker as two
/// columns, so a sequence element's keys sit past it.
fn line_indent(line: &str) -> usize {
    let trimmed = line.trim_start();
    let spaces = line.len() - trimmed.len();
    if trimmed == "-" || trimmed.starts_with("- ") {
        spaces + 2
    } else {
        spaces
    }
}

/// Parses a line into the key it declares, its column, and whether it opens a
/// block scalar. Returns `None` for a line that declares no mapping key, such as
/// a scalar sequence item or a comment.
fn parse_key(line: &str, indent: usize) -> Option<(usize, String, bool)> {
    let mut rest = &line[indent..];
    let mut col = indent;
    if let Some(after) = rest.strip_prefix("- ") {
        let extra = after.len() - after.trim_start().len();
        col += 2 + extra;
        rest = after.trim_start();
    } else if rest == "-" {
        return None;
    }
    let colon = find_top_colon(rest)?;
    let after = &rest[colon + 1..];
    if !(after.is_empty() || after.starts_with(' ')) {
        return None;
    }
    let name = rest[..colon].trim().trim_matches('"');
    if name.is_empty() {
        return None;
    }
    let value = after.trim_start();
    let opens_scalar = value.starts_with('|') || value.starts_with('>');
    Some((col, name.to_string(), opens_scalar))
}

/// The offset of the first `:` in a segment that is not inside a quoted string,
/// so a `:` inside a value such as `http://host` is not read as the separator.
fn find_top_colon(segment: &str) -> Option<usize> {
    let bytes = segment.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_double(bytes, index),
            b'\'' => index = skip_single(bytes, index),
            b':' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

/// The index past a double-quoted string that starts at `open`, honoring `\"`.
fn skip_double(bytes: &[u8], open: usize) -> usize {
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

/// The index past a single-quoted string that starts at `open`.
fn skip_single(bytes: &[u8], open: usize) -> usize {
    let mut index = open + 1;
    while index < bytes.len() {
        if bytes[index] == b'\'' {
            return index + 1;
        }
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::PositionKind;

    /// The offset of the cursor marker `|` in a fixture, with the marker removed.
    fn at(marked: &str) -> (String, usize) {
        let offset = marked.find('|').expect("a cursor marker");
        let text = format!("{}{}", &marked[..offset], &marked[offset + 1..]);
        (text, offset)
    }

    #[test]
    fn a_fresh_line_under_a_populated_mapping_is_a_body_in_that_mapping() {
        // Arrange
        let (text, offset) = at("limits:\n  mode: enforce\n  |\n");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_fresh_line_under_an_empty_key_is_a_body_in_that_key() {
        // Arrange
        // The value-on-next-line case: `limits:` awaits its body, and the cursor
        // is on the indented line below.
        let (text, offset) = at("limits:\n  |\n");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_cursor_in_a_block_sequence_element_resolves_into_the_element() {
        // Arrange
        let (text, offset) = at("rules:\n  - prefix: /api\n    |\n");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, vec!["rules".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_value_on_the_sequence_element_line_is_an_attribute_value() {
        // Arrange
        let (text, offset) = at("rules:\n  - prefix: /a|\n");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, vec!["rules".to_string()]);
        assert_eq!(
            context.kind,
            PositionKind::AttributeValue {
                field: "prefix".to_string()
            }
        );
    }

    #[test]
    fn an_inner_colon_in_a_value_is_not_the_separator() {
        // Arrange
        let (text, offset) = at("headers:\n  url: http://host|\n");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, vec!["headers".to_string()]);
        assert_eq!(
            context.kind,
            PositionKind::AttributeValue {
                field: "url".to_string()
            }
        );
    }

    #[test]
    fn a_block_scalar_body_is_not_read_as_structure() {
        // Arrange
        // The `note: |` block scalar holds `nested: text`, which must not be read
        // as a key, so the cursor under `port` resolves to the root. The cursor
        // marker is not used here, because the fixture holds a literal `|`.
        let text = "note: |\n  nested: text\nport\n";
        let offset = text.find("port").unwrap() + "port".len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_cursor_two_levels_deep_collects_both_keys() {
        // Arrange
        let (text, offset) = at("a:\n  b:\n    c: 1\n    |\n");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_value_position_reads_the_key_before_the_colon() {
        // Arrange
        let (text, offset) = at("limits:\n  mode: enf|\n");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(
            context.kind,
            PositionKind::AttributeValue {
                field: "mode".to_string()
            }
        );
    }

    #[test]
    fn crlf_line_endings_do_not_collapse_the_path() {
        // Arrange
        // A Windows-authored file uses CRLF, so each parent key line ends `\r`.
        // The reader must still read the enclosing path.
        let text = "limits:\r\n  mode: enforce\r\n  \r\n";
        let offset = text.rfind('\r').expect("a carriage return");

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_multi_line_flow_collection_is_not_read_as_structure() {
        // Arrange
        // `headers` is a multi-line flow map, so its inner keys nest inline, not
        // by indentation, and must not become ancestors of the cursor below it.
        let text = "headers: {\n  x-env: prod\n}\nport\n";
        let offset = text.find("port").unwrap() + "port".len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_block_sequence_with_the_dash_at_the_key_indent_resolves_into_the_element() {
        // Arrange
        // A block sequence may place the dash at the parent key's indentation
        // rather than indented under it, so both forms resolve into the element.
        let text = "rules:\n- prefix: /api\n  \n";
        let offset = text.len() - 1;

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, vec!["rules".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_block_scalar_that_dedents_resets_and_the_key_after_is_read() {
        // Arrange
        // The `note: |` block scalar holds `body`, then `top:` dedents back to the
        // header column, which ends the scalar so `top` is read as a real key.
        let text = "note: |\n  body\ntop:\n  ";
        let offset = text.len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, vec!["top".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_negative_flow_balance_is_clamped_to_zero() {
        // Arrange
        // A stray closing brace drives the running flow balance below zero, which
        // must clamp to zero so the following key is still read as structure.
        let text = "foo: }\nbar:\n  ";
        let offset = text.len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, vec!["bar".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn flow_balance_skips_brackets_inside_quotes() {
        // Arrange
        // The bracket in each quoted value must not change the flow balance, so the
        // key below is still read as an ancestor.
        let text = "a: \"x{y\"\nb: 'p[q'\nc:\n  ";
        let offset = text.len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, vec!["c".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_colon_with_no_following_space_is_a_body_position() {
        // Arrange
        let (text, offset) = at("foo:bar|");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_bare_dash_line_is_not_read_as_a_key() {
        // Arrange
        // A bare `-` sequence marker with no inline content declares no mapping key,
        // so it is skipped and does not become an ancestor.
        let text = "list:\n-\n";
        let offset = text.len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn an_ancestor_line_with_no_space_after_the_colon_is_not_a_key() {
        // Arrange
        // The `a:bcd` line packs a value against the colon, so it declares no
        // mapping key and only `b` is read as an ancestor.
        let text = "a:bcd\nb:\n  ";
        let offset = text.len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, vec!["b".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn an_ancestor_line_with_an_empty_key_is_not_a_key() {
        // Arrange
        // The `: x` line has nothing before the colon, so it declares no key and
        // only `k` is read as an ancestor.
        let text = ": x\nk:\n  ";
        let offset = text.len();

        // Act
        let context = resolve_in_yaml(text, offset);

        // Assert
        assert_eq!(context.path, vec!["k".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn a_double_quoted_key_with_an_inner_colon_reads_the_real_colon() {
        // Arrange
        // The quoted key holds an escaped quote and a colon, both of which the
        // scanner steps over to find the separator that follows the closing quote.
        let (text, offset) = at("\"a\\\"b:c\": v|");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(
            context.kind,
            PositionKind::AttributeValue {
                field: "a\\\"b:c".to_string()
            }
        );
    }

    #[test]
    fn a_single_quoted_key_with_an_inner_colon_reads_the_real_colon() {
        // Arrange
        let (text, offset) = at("'a:b': v|");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(
            context.kind,
            PositionKind::AttributeValue {
                field: "'a:b'".to_string()
            }
        );
    }

    #[test]
    fn an_unterminated_double_quote_is_a_body_position() {
        // Arrange
        // With no closing quote there is no top-level colon, so the line is a body
        // position rather than an attribute value.
        let (text, offset) = at("\"abc|");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }

    #[test]
    fn an_unterminated_single_quote_is_a_body_position() {
        // Arrange
        let (text, offset) = at("'abc|");

        // Act
        let context = resolve_in_yaml(&text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }
}
