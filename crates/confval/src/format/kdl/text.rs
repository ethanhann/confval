//! The value forms of the KDL write path: the quoted string form, the
//! scalar entry text, and the bare-name check.
//!
//! These decide the exact text a value renders as, kept apart from the
//! document assembly in [`emit`](super::emit_kdl)'s module so each half stays
//! readable on its own.

use crate::format::field::Scalar;
use kdl::{KdlEntry, KdlEntryFormat, KdlValue};

/// Whether a name can be written bare. The check is narrower than KDL's own identifier
/// grammar, so a borderline name quotes rather than risking text the
/// parser reads differently.
pub(super) fn is_plain_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// One argument entry, with the canonical text set explicitly, because
/// kdl-rs's own rendering writes an identifier-shaped string bare and the
/// canonical form keeps every string quoted.
pub(super) fn scalar_entry(scalar: &Scalar) -> KdlEntry {
    let (value, repr) = match scalar {
        Scalar::String(string) => (KdlValue::String(string.clone()), quoted(string)),
        Scalar::Int(int) => (KdlValue::Integer(i128::from(*int)), int.to_string()),
        Scalar::Float(float) => (KdlValue::Float(*float), float_repr(*float)),
        Scalar::Bool(boolean) => (KdlValue::Bool(*boolean), format!("#{boolean}")),
        Scalar::Unparsed(raw) => (KdlValue::String(raw.clone()), quoted(raw)),
    };
    let mut entry = KdlEntry::new(value);
    entry.set_format(KdlEntryFormat {
        value_repr: repr,
        leading: " ".to_string(),
        ..KdlEntryFormat::default()
    });
    entry
}

/// The quoted form of a string, with the escapes KDL 2.0 defines and a
/// unicode escape for every code point its grammar bans from literal text, so
/// an adversarial string still reparses.
pub(super) fn quoted(string: &str) -> String {
    let mut out = String::with_capacity(string.len() + 2);
    out.push('"');
    for character in string.chars() {
        match character {
            '\\' | '"' => {
                out.push('\\');
                out.push(character);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            character if character.is_control() || is_banned_in_text(character) => {
                out.push_str(&format!("\\u{{{:x}}}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// Whether KDL 2.0 bans the code point from appearing literally in its text:
/// the direction marks, the bidi controls, the zero-width no-break space, and
/// the two line separators it treats as newlines, which end a single-line
/// string early.
fn is_banned_in_text(character: char) -> bool {
    matches!(
        character,
        '\u{200E}' | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// A float's literal: the shortest round-trip text for a finite value, which
/// always carries a decimal point or an exponent, and the KDL 2.0 keyword for
/// a non-finite one.
fn float_repr(float: f64) -> String {
    if float == f64::INFINITY {
        "#inf".to_string()
    } else if float == f64::NEG_INFINITY {
        "#-inf".to_string()
    } else if float.is_nan() {
        "#nan".to_string()
    } else {
        format!("{float:?}")
    }
}
