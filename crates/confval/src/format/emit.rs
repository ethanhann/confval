//! Shared support for the format emitters: the error they return and the
//! normalization that keeps a doc comment safe to render.
//!
//! Emit serializes the neutral [`Fields`](crate::format::Fields) model back to a
//! format's text. Not every `Fields` is representable in every format, so emit
//! is fallible. Emitting a populated spec to TOML never fails, because TOML has a
//! literal for every value populate produces and quotes any key. Emitting a
//! populated spec to HCL fails only for the two numeric values HCL cannot spell,
//! an `i64::MIN` and a non-finite float. The remaining failures arise when you
//! emit a `Fields` a frontend parsed, which can carry a name or a value the
//! target format cannot spell.

use std::fmt::{self, Display, Formatter};

/// Why a `Fields` could not be emitted to a format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// A name has no representation in the target format, such as a
    /// non-identifier attribute or block name in HCL. TOML quotes any key, so
    /// this arises only for HCL.
    UnrepresentableName(String),
    /// A `ValueKind::Other`, a value the neutral model could not represent such
    /// as an HCL template or a TOML datetime, so there is no literal to emit.
    /// The string is the model's noun for it.
    UnrepresentableValue(&'static str),
}

impl Display for EmitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EmitError::UnrepresentableName(name) => {
                write!(
                    f,
                    "cannot emit `{name}`: not a valid name in the target format"
                )
            }
            EmitError::UnrepresentableValue(label) => {
                write!(
                    f,
                    "cannot emit {label}: the value has no representation in the model"
                )
            }
        }
    }
}

impl std::error::Error for EmitError {}

/// Splits a doc comment into the lines to render as `#` comments, so a doc from
/// any source is safe to emit into either format.
///
/// A `///` comment is already clean, but a `#[confval(doc = "...")]` override or
/// a hand-built `with_doc` can carry any character. Each line-break variant
/// becomes its own line, so a stray carriage return reads as the break it stands
/// for. A control character other than tab is dropped, because HCL ends a comment
/// at a bare carriage return and TOML rejects a non-printable one.
pub(crate) fn comment_lines(doc: &str) -> Vec<String> {
    doc.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|line| {
            line.chars()
                .filter(|&character| character == '\t' || !character.is_control())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_lines_splits_every_line_break_variant() {
        assert_eq!(comment_lines("a\r\nb"), vec!["a", "b"]);
        assert_eq!(comment_lines("a\rb"), vec!["a", "b"]);
        assert_eq!(comment_lines("a\nb"), vec!["a", "b"]);
    }

    #[test]
    fn comment_lines_strips_controls_but_keeps_tab() {
        assert_eq!(comment_lines("a\u{0}b"), vec!["ab"]);
        assert_eq!(comment_lines("a\tb"), vec!["a\tb"]);
    }

    #[test]
    fn comment_lines_keeps_empty_lines() {
        assert_eq!(comment_lines(""), vec![""]);
        assert_eq!(comment_lines("a\n\nb"), vec!["a", "", "b"]);
    }
}
