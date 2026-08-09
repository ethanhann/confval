//! The text mechanics of the YAML write path: how one scalar, one key, and one
//! string are written, and how a rendered block gains its sequence marker or
//! its comment marker.
//!
//! Every string writes double-quoted, so text the core schema would resolve to
//! something else, `no` or `123` or `null`, reads back as the string it was. A
//! key is a name rather than a typed value, so it writes bare whenever it is an
//! ASCII identifier.

use crate::format::field::Scalar;

/// Writes a rendered element body with its first content line carrying the `- `
/// marker, so a mapping element opens on the marker's line.
///
/// A doc comment above that first entry keeps its own indentation and renders
/// before the marker, because a marker inside a comment would hide the element.
pub(super) fn splice_dash(out: &mut String, body: &str, level: usize) {
    let column = level * 2;
    let mut spliced = false;
    for line in body.lines() {
        let content = line.trim_start();
        if !spliced && !content.is_empty() && !content.starts_with('#') && line.len() >= column + 2
        {
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
pub(super) fn comment_out(body: &str) -> String {
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
pub(super) fn write_scalar(out: &mut String, scalar: &Scalar) {
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
/// exponent, so the resolution never reads it as an integer. YAML 1.2 has a
/// literal for each of the three non-finite values.
pub(super) fn float_text(float: f64) -> String {
    if float.is_nan() {
        return ".nan".to_string();
    }
    if float.is_infinite() {
        return if float.is_sign_negative() {
            "-.inf"
        } else {
            ".inf"
        }
        .to_string();
    }
    format!("{float:?}")
}

/// Writes a key bare when it is an ASCII identifier, and double-quoted
/// otherwise.
pub(super) fn write_key(out: &mut String, name: &str) {
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
pub(super) fn write_string(out: &mut String, text: &str) {
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
pub(super) fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}
