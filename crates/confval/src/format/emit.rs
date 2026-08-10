//! Shared support for the format emitters: the error they return and the
//! normalization that keeps a doc comment safe to render.
//!
//! Emit serializes the neutral [`Fields`](crate::format::Fields) model back to a
//! format's text. Not every `Fields` is representable in every format, so emit
//! is fallible. Emitting a populated spec to TOML never fails, because TOML has a
//! literal for every value populate produces and quotes any key. Emitting a
//! populated spec to HCL fails only for the two numeric values HCL cannot write,
//! an `i64::MIN` and a non-finite float. Emitting a populated spec to JSON fails
//! only for a non-finite float. Emitting a populated spec to KDL or YAML never
//! fails either, because each has a literal for every value populate produces
//! and quotes any key. The remaining failures arise when you
//! emit a `Fields` a frontend parsed or built by hand, which can carry a name
//! or a value the target format cannot write, or use one name in conflicting
//! ways at one level, such as a value next to a same-named block in TOML, JSON,
//! or YAML, or two same-named values in HCL or TOML.
//!
//! A layered tree can carry [`Scalar::Unparsed`](super::Scalar) text from an
//! environment variable or a flag. That text emits as a string literal, since
//! its type was never decided, so a typed reparse of the emitted file reads
//! those leaves as strings.

#[cfg(any(feature = "toml", feature = "hcl", feature = "json", feature = "yaml"))]
use super::field::{Entry, Field, FieldKind, Fields};
#[cfg(any(feature = "json", feature = "yaml"))]
use super::field::{Value, ValueKind};
use std::fmt::{self, Display, Formatter};

/// Why a `Fields` could not be emitted to a format.
///
/// Each variant carries `path`, the dotted field path to the problem, so an error in a large tree names its location. For a name problem
/// the path is the enclosing level, empty at the document root. For a value
/// problem it is the offending field itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// A name has no representation in the target format, such as a
    /// non-identifier attribute or block name in HCL. TOML quotes any key, so
    /// this arises only for HCL.
    UnrepresentableName {
        /// The name that cannot be written.
        name: String,
        /// The dotted path of the enclosing level, empty at the root.
        path: String,
    },
    /// A `ValueKind::Other`, a value the neutral model could not represent such
    /// as an HCL template or a TOML datetime, so there is no literal to emit.
    /// The label is the model's noun for it.
    UnrepresentableValue {
        /// The model's noun for the value, such as "datetime".
        label: &'static str,
        /// The dotted path of the field holding the value.
        path: String,
    },
    /// A name used at one level in a way the target format cannot write twice:
    /// two values under one name in HCL or TOML, a value next to a block in
    /// TOML or JSON, or any repetition inside an inline table or object. Emitting
    /// would silently lose one of the uses, so emit refuses. Populate never
    /// produces these, so they arise only for a parsed or hand-built `Fields`.
    ConflictingName {
        /// The name with conflicting uses.
        name: String,
        /// The dotted path of the enclosing level, empty at the root.
        path: String,
    },
}

/// The location suffix for an emit error message, empty at the document root.
fn location(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!(" (at `{path}`)")
    }
}

/// The dotted path of a field under `path`, which is empty at the root.
#[cfg(any(
    feature = "toml",
    feature = "hcl",
    feature = "kdl",
    feature = "json",
    feature = "yaml"
))]
pub(crate) fn child_path(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

/// A level's fields in emit order: every value, then every block, each group in
/// declaration order.
///
/// HCL follows the Terraform convention of arguments before nested blocks.
/// TOML's syntax forces the same order, because a bare key written after a
/// table header would belong to that table. JSON and YAML impose no order of
/// their own. Their emitters follow the same one, so the four formats read
/// alike, and one walk serves all of them.
///
/// This yields entries rather than fields, because an emitter renders the
/// commented ones too and places each in the region its active twin would
/// occupy.
#[cfg(any(feature = "toml", feature = "hcl", feature = "json", feature = "yaml"))]
pub(crate) fn values_then_blocks(fields: &Fields) -> impl Iterator<Item = &Entry> {
    let values = fields
        .entries()
        .filter(|entry| matches!(entry.field().kind, FieldKind::Value(_)));
    let blocks = fields
        .entries()
        .filter(|entry| matches!(entry.field().kind, FieldKind::Block(_)));
    values.chain(blocks)
}

/// The first name at this level whose same-named group `rejects`.
///
/// Each format refuses some repetition it cannot write, and the formats differ
/// only in which groups they refuse. `rejects` receives the fields sharing one
/// name, in declaration order. A commented entry is comment text, so it never
/// reaches here and conflicts with nothing.
#[cfg(any(feature = "toml", feature = "hcl", feature = "json", feature = "yaml"))]
pub(crate) fn first_conflicting_name(
    fields: &Fields,
    rejects: impl Fn(&[&Field]) -> bool,
) -> Option<&str> {
    fields.iter().find_map(|field| {
        let group: Vec<&Field> = fields
            .iter()
            .filter(|other| other.name == field.name)
            .collect();
        rejects(&group).then_some(field.name.as_str())
    })
}

/// The elements a repeated value field contributes to one grouped sequence.
///
/// A sequence occurrence contributes its elements in document order, and a
/// scalar or a nested level contributes itself as one element. This is the
/// accumulation the generated walk performs, so a list-shaped field reads the
/// same resolved list from the grouped member that it would have read from the
/// separate fields.
///
/// The formats that group a repeated name rather than refusing it, JSON and
/// YAML, share this rule, because it belongs to the neutral model rather than
/// to either syntax.
#[cfg(any(feature = "json", feature = "yaml"))]
pub(crate) fn grouped_elements<'a>(group: &[&'a Value]) -> Vec<&'a Value> {
    let mut elements: Vec<&Value> = Vec::new();
    for value in group {
        match &value.kind {
            ValueKind::Seq(inner) => elements.extend(inner.iter()),
            _ => elements.push(value),
        }
    }
    elements
}

/// The first name at this level used both as a value and as a block.
///
/// A format whose only way to write the pair is a duplicate key refuses it
/// rather than losing one of the two. JSON and YAML are both such formats.
#[cfg(any(feature = "json", feature = "yaml"))]
pub(crate) fn value_beside_block(fields: &Fields) -> Option<&str> {
    first_conflicting_name(fields, |group| {
        group
            .iter()
            .any(|field| matches!(field.kind, FieldKind::Value(_)))
            && group
                .iter()
                .any(|field| matches!(field.kind, FieldKind::Block(_)))
    })
}

/// Any name repeated at a level with unique keys, an HCL object or a
/// TOML inline table, where no syntax carries a repetition.
#[cfg(any(feature = "toml", feature = "hcl"))]
pub(crate) fn repeated_name(fields: &Fields) -> Option<&str> {
    first_conflicting_name(fields, |group| group.len() > 1)
}

impl Display for EmitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            EmitError::UnrepresentableName { name, path } => {
                write!(
                    f,
                    "cannot emit `{name}`: not a valid name in the target format{}",
                    location(path)
                )
            }
            EmitError::UnrepresentableValue { label, path } => {
                write!(
                    f,
                    "cannot emit {label}: the value has no representation in the model{}",
                    location(path)
                )
            }
            EmitError::ConflictingName { name, path } => {
                write!(
                    f,
                    "cannot emit `{name}`: the name has conflicting uses at one level{}",
                    location(path)
                )
            }
        }
    }
}

impl std::error::Error for EmitError {}

/// Splits a doc comment into the lines each format renders as its own comment,
/// so a doc from any source is safe to emit into any of them.
///
/// A `///` comment is already clean, but a `#[confval(doc = "...")]` override or
/// a hand-built `with_doc` can carry any character. Each line-break variant
/// becomes its own line, so a stray carriage return reads as the break it stands
/// for. A control character other than tab is dropped, because HCL ends a comment
/// at a bare carriage return, TOML rejects a non-printable one, and KDL bans
/// several from its text.
///
/// The caller supplies the marker. HCL, TOML, and YAML write `#`, and KDL
/// writes `//`.
#[cfg(any(feature = "toml", feature = "hcl", feature = "kdl", feature = "yaml"))]
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

#[cfg(all(test, any(feature = "toml", feature = "hcl")))]
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
