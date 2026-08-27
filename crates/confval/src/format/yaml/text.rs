//! The text mechanics of the YAML write path: how one scalar, one key, and one
//! string are written, and how a rendered block gains its sequence marker or
//! its comment marker.
//!
//! Every string writes double-quoted, so text the core schema would resolve to
//! something else, `no` or `123` or `null`, reads back as the string it was. A
//! key is a name rather than a typed value, so it writes bare whenever it is an
//! ASCII identifier.

use crate::format::emit::indent;
use crate::format::field::Scalar;

/// Writes a rendered element body with its first content line holding the `- `
/// marker, so a mapping element opens on the marker's line.
///
/// A doc comment above that first entry keeps its own indentation and renders
/// before the marker, because a marker inside a comment would hide the element.
///
/// A body with no line the marker can take gets `- {}` on a line of its own,
/// with the body below it. That is an empty body, which a repeated empty block
/// produces, and one holding only comments, which a template produces for an
/// element that sets nothing. Writing the body unmarked would drop the element
/// from the sequence with no diagnostic.
pub(super) fn splice_dash(out: &mut String, body: &str, level: usize) {
    let column = level * 2;
    let mut spliced = false;
    let mut lines = String::new();
    for line in body.lines() {
        let content = line.trim_start();
        if !spliced && !content.is_empty() && !content.starts_with('#') && line.len() >= column + 2
        {
            lines.push_str(&line[..column]);
            lines.push_str("- ");
            lines.push_str(&line[column + 2..]);
            spliced = true;
        } else {
            lines.push_str(line);
        }
        lines.push('\n');
    }
    if !spliced {
        indent(out, level);
        out.push_str("- {}\n");
    }
    out.push_str(&lines);
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
    if is_plain_name(name) {
        out.push_str(name);
    } else {
        write_string(out, name);
    }
}

/// Whether a key is plainly safe: ASCII letters, digits, `_`, and `-`, opening
/// with a letter or `_`. The check is narrow, because a key that
/// resolves to something other than a string would change meaning.
fn is_plain_name(name: &str) -> bool {
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
///
/// This body is a coincidental duplicate of `json::emit::write_string`, not a
/// shared source. YAML's double-quoted repertoire is broader than JSON's RFC
/// 8259 set, so the two are free to diverge.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_an_ascii_identifier() {
        // Arrange
        let plain = ["port", "max_body-mb", "_ok", "a1"];
        let quoted = ["9lives", "weird key", "", "a.b", "kéy", "-lead"];

        // Act
        let answers: Vec<bool> = plain
            .iter()
            .chain(&quoted)
            .map(|n| is_plain_name(n))
            .collect();

        // Assert
        assert_eq!(answers[..plain.len()], [true; 4], "plain: {plain:?}");
        assert!(
            answers[plain.len()..].iter().all(|answer| !answer),
            "quoted: {quoted:?}"
        );
    }

    #[test]
    fn a_float_always_carries_a_point_or_an_exponent() {
        // Arrange
        // The core schema reads a bare digit string as an integer, so a float
        // that wrote one would change kind on reparse.
        let values = [1.0, 0.5, 1e20, -1e-7, f64::MAX];

        // Act
        let written: Vec<String> = values.iter().map(|value| float_text(*value)).collect();

        // Assert
        for (value, text) in values.iter().zip(&written) {
            assert!(
                text.contains(['.', 'e', 'E']),
                "{value} wrote {text}, which reads back as an integer"
            );
        }
    }

    #[test]
    fn a_non_finite_float_writes_its_yaml_literal() {
        // Arrange
        let values = [f64::INFINITY, f64::NEG_INFINITY, f64::NAN];

        // Act
        let written: Vec<String> = values.iter().map(|value| float_text(*value)).collect();

        // Assert
        assert_eq!(written, vec![".inf", "-.inf", ".nan"]);
    }

    #[test]
    fn the_short_escapes_are_used_where_they_exist() {
        // Arrange
        let mut out = String::new();

        // Act
        write_string(
            &mut out,
            "q\" b\\ nl\n tab\t cr\r bs\u{8} ff\u{c} unit\u{1f} raw\u{2603}",
        );

        // Assert
        assert_eq!(
            out,
            "\"q\\\" b\\\\ nl\\n tab\\t cr\\r bs\\b ff\\f unit\\u001f raw\u{2603}\""
        );
    }

    #[test]
    fn comment_out_puts_the_marker_after_the_indentation() {
        // Arrange
        // Uncommenting is deleting the marker, so the entry must keep its
        // column.
        let body = "limits:\n  mode: \"log\"\n";
        let mut out = String::new();

        // Act
        out.push_str(&comment_out(body));

        // Assert
        assert_eq!(out, "#limits:\n  #mode: \"log\"\n");
    }

    #[test]
    fn splice_dash_marks_the_first_content_line() {
        // Arrange
        // The body renders one level deeper than the marker, and a doc comment
        // above the first entry keeps its own column.
        let body = "    # The port.\n    port: 1\n    host: \"h\"\n";
        let mut out = String::new();

        // Act
        splice_dash(&mut out, body, 1);

        // Assert
        assert_eq!(out, "    # The port.\n  - port: 1\n    host: \"h\"\n");
    }

    #[test]
    fn splice_dash_keeps_an_element_whose_body_offers_no_target() {
        // Arrange
        // An empty body and one holding only comments both reach here, and
        // writing either unmarked would drop the element from the sequence.
        let mut empty = String::new();
        let mut commented = String::new();

        // Act
        splice_dash(&mut empty, "", 1);

        // Assert
        splice_dash(&mut commented, "    #port: 1\n", 1);
        assert_eq!(empty, "  - {}\n");
        assert_eq!(commented, "  - {}\n    #port: 1\n");
    }
}
