//! JSON structural recovery from raw text, for a buffer that does not parse.
//!
//! It reconstructs the enclosing object path from the open braces and brackets
//! and the property key before the cursor, so completion still resolves inside
//! the object or the array element the cursor sits in while the buffer is broken.

use crate::text_scan::skip_string;

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
