//! JSON structural recovery from raw text, for a buffer that does not parse.
//!
//! It reconstructs the enclosing object path from the open braces and brackets
//! and the property key before the cursor, so completion still resolves inside
//! the object or the array element the cursor is in while the buffer is broken.

use super::text::skip_string;

/// The enclosing object path in a JSON document: the keys of the objects whose
/// braces are open at the offset. An array element's object contributes no key,
/// so a cursor inside `"rules": [ { … } ]` collects `rules` alone, matching the
/// clean walk's array-element entry.
pub(crate) fn object_path(text: &str, offset: usize) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<String> = Vec::new();
    let mut index = 0;
    while index < offset {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index),
            b'{' | b'[' => {
                stack.push(introducing_key(text, index));
                index += 1;
            }
            b'}' | b']' => {
                stack.pop();
                index += 1;
            }
            _ => index += 1,
        }
    }
    stack.into_iter().filter(|key| !key.is_empty()).collect()
}

/// The key that introduces an object or array: the quoted key of a `"key": {`
/// or `"key": [`, or the empty string for an array element or the root, which
/// no key names.
fn introducing_key(text: &str, open: usize) -> String {
    let bytes = text.as_bytes();
    let mut index = open;
    while index > 0 && bytes[index - 1].is_ascii_whitespace() {
        index -= 1;
    }
    if index == 0 || bytes[index - 1] != b':' {
        return String::new();
    }
    json_key(&text[..index - 1]).unwrap_or_default()
}

/// Whether the innermost open bracket at the offset is a JSON array, so the
/// cursor is directly in an array rather than inside an element object. This
/// is the JSON half of the new-element answer the frontend resolves onto the
/// cursor context.
///
/// Brackets match by kind, so an unpaired closer in a malformed buffer, the
/// common state during completion, does not pop a bracket it never closed.
pub(crate) fn innermost_is_array(text: &str, offset: usize) -> bool {
    let bytes = text.as_bytes();
    let mut stack = Vec::new();
    let mut index = 0;
    while index < offset {
        match bytes[index] {
            b'"' => index = skip_string(bytes, index),
            b'{' | b'[' => {
                stack.push(bytes[index]);
                index += 1;
            }
            b'}' => {
                if stack.last() == Some(&b'{') {
                    stack.pop();
                }
                index += 1;
            }
            b']' => {
                if stack.last() == Some(&b'[') {
                    stack.pop();
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    matches!(stack.last(), Some(b'['))
}

/// The content of the last quoted string in a segment, honoring `\"`.
fn json_key(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut index = 0;
    let mut last = None;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let end = skip_string(bytes, index);
            if end > index + 1 && end <= bytes.len() {
                last = Some(segment[index + 1..end - 1].to_string());
            }
            index = end;
        } else {
            index += 1;
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_key_ignores_an_unterminated_trailing_quote() {
        // Arrange
        // The segment holds a complete key `a`, then an unterminated quote as its
        // last byte. `skip_string` returns `bytes.len()`, so the trailing quote's
        // `end` equals its index plus one. The guard must reject that run rather
        // than slice a reversed range, so the last real key stays `a`.
        let segment = "\"a\" \"";

        // Act
        let last = json_key(segment);

        // Assert
        assert_eq!(last, Some("a".to_string()));
    }

    #[test]
    fn an_unpaired_closing_brace_does_not_pop_the_open_array() {
        // Arrange
        // The `}` never closed the `[`, so the cursor is still directly in
        // the array and a completion wraps a new element.
        let text = "[ }";

        // Act
        let in_array = innermost_is_array(text, text.len());

        // Assert
        assert!(in_array, "the unpaired closer leaves the array open");
    }

    #[test]
    fn a_cursor_inside_an_element_object_is_not_directly_in_the_array() {
        // Arrange
        let text = "[ { ";

        // Act
        let in_array = innermost_is_array(text, text.len());

        // Assert
        assert!(!in_array, "the element object is the innermost bracket");
    }

    #[test]
    fn object_path_skips_a_closing_brace_inside_a_quoted_key() {
        // Arrange
        // The `}` sits inside the quoted key `"k}"`, so it must not close the
        // outer object and the enclosing key stays `outer`.
        let text = "{\"outer\": {\"k}\": 1";

        // Act
        let path = object_path(text, text.len());

        // Assert
        assert_eq!(path, vec!["outer".to_string()]);
    }

    #[test]
    fn object_path_pops_a_real_closing_brace() {
        // Arrange
        // The inner object closes before the cursor, so its key `a` is not an
        // ancestor of the position after it.
        let text = "{\"a\": {\"b\": 1}, \"c\": ";

        // Act
        let path = object_path(text, text.len());

        // Assert
        assert_eq!(path, Vec::<String>::new());
    }

    #[test]
    fn innermost_is_array_skips_brackets_inside_a_string() {
        // Arrange
        // The two `]` sit inside the quoted value, so neither closes the array
        // and the cursor is still directly in it.
        let text = "[ \"]]\" ";

        // Act
        let in_array = innermost_is_array(text, text.len());

        // Assert
        assert!(in_array, "quoted brackets leave the array open");
    }

    #[test]
    fn innermost_is_array_pops_a_matching_array_close() {
        // Arrange
        // The array closes inside the object, so the innermost open bracket is
        // the object brace, not the array.
        let text = "{ [ ] ";

        // Act
        let in_array = innermost_is_array(text, text.len());

        // Assert
        assert!(!in_array, "the closed array leaves the object innermost");
    }

    #[test]
    fn innermost_is_array_does_not_pop_an_array_across_an_object() {
        // Arrange
        // The `]` does not match the innermost `{`, so it pops nothing and the
        // object stays the innermost open bracket.
        let text = "[ { ] ";

        // Act
        let in_array = innermost_is_array(text, text.len());

        // Assert
        assert!(!in_array, "a mismatched close pops nothing");
    }
}
