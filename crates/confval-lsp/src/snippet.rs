//! The completion snippet grammar, owned in one place.
//!
//! The grammar is a closed list: a `$n` tab stop and a `${n:value}`
//! placeholder with backslash escaping inside the value. The frontends
//! produce it through [`escape`], and the completion encoder removes it
//! through [`strip`] for a client without snippet support. A producer adding
//! a new snippet form extends this module first.

/// Escapes the snippet metacharacters, so user text passes through a
/// placeholder verbatim.
pub(crate) fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '$' | '}' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Removes the snippet markers for a client without snippet support. A `$n`
/// tab stop is dropped, and a `${n:value}` placeholder unwraps to its value
/// with the backslash escaping removed, so the bare text reaches the buffer.
pub(crate) fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '$' {
            out.push(character);
            continue;
        }
        match chars.peek() {
            Some(digit) if digit.is_ascii_digit() => {
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                }
            }
            Some('{') => {
                chars.next();
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                }
                if chars.peek() == Some(&':') {
                    chars.next();
                }
                while let Some(inner) = chars.next() {
                    match inner {
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                out.push(escaped);
                            }
                        }
                        '}' => break,
                        other => out.push(other),
                    }
                }
            }
            _ => out.push('$'),
        }
    }
    out
}
