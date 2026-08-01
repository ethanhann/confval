//! The format-neutral field model.
//!
//! Every concrete format (HCL, TOML, KDL) parses its own syntax tree and
//! then lowers it into the owned types defined here: a [`Fields`] is one
//! structural level (a body, a table, an inline object), each [`Field`] is one
//! named entry, and a [`Value`] is the data behind it. Once a frontend has
//! produced a `Fields`, nothing downstream knows or cares which format it came
//! from. This holds for the leaf parsers in [`parse`](crate::format::parse),
//! the `#[derive(Spec)]`-generated walks, and the handwritten [`FromFields`]
//! impls.
//!
//! The model is deliberately owned (no borrow of the format's AST). Config
//! files are small, so the one copy out of the parse tree is cheap and removes
//! every dependence on one format's node types.

use crate::diagnostic::Report;
use crate::source::{SourceId, Span};

/// A scalar leaf: the value kinds every supported format shares, plus the raw
/// form a non-file source yields.
///
/// Integers and floats are kept distinct so a format that separates them
/// syntactically (TOML's `1` vs `1.0`) round-trips faithfully. A format with a
/// single number type (HCL) classifies each literal as one or the other.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    /// A string value.
    String(String),
    /// An integer value.
    Int(i64),
    /// A float value.
    Float(f64),
    /// A boolean value.
    Bool(bool),
    /// A raw string from a source that carries only strings, such as an
    /// environment variable or a command line flag, before it is parsed to a
    /// type. The leaf parsers coerce it to the type they expect, so the field's
    /// declared type decides rather than a guess from the text. No file
    /// frontend produces it, so a quoted string in a file stays a
    /// [`String`](Scalar::String).
    Unparsed(String),
}

/// The data behind a field, with the span it occupied in source.
#[derive(Debug, Clone)]
pub struct Value {
    /// The byte range the value occupied in its source.
    pub span: Span,
    /// What the value is.
    pub kind: ValueKind,
}

/// A value is a scalar, a sequence, a nested structure, or something the
/// model cannot represent.
#[derive(Debug, Clone)]
pub enum ValueKind {
    /// A single leaf value.
    Scalar(Scalar),
    /// An array. Elements keep their own spans so a bad element is reported at
    /// the element, not the whole list.
    Seq(Vec<Value>),
    /// A nested structure spelled inline (an HCL object, a TOML inline table).
    Map(Fields),
    /// Present in source but outside the model: an HCL template or null, a
    /// TOML datetime. The label is the noun diagnostics use ("string
    /// template", "datetime"). No leaf parser matches it, so it always
    /// surfaces as a type mismatch.
    Other(&'static str),
}

/// One named entry at a structural level: an attribute, a block, or a table.
#[derive(Debug, Clone)]
pub struct Field {
    /// The field's name as written in the source.
    pub name: String,
    /// Span of the name alone (attribute key, block identifier, table header
    /// key), where unknown-field errors point.
    pub name_span: Span,
    /// Span of the whole field, name and value together.
    pub span: Span,
    /// The source the field was read from.
    pub source: SourceId,
    /// Whether the field is an attribute or a block, and its data.
    pub kind: FieldKind,
    /// The doc comment to render above the field when emitting an annotated
    /// template, or `None` for no comment. Parsing sets it to `None`, because a
    /// parsed file's comments are dropped. The populate walk sets it from a
    /// spec field's doc comment for `to_template`. A multi-line comment is one
    /// string with newline separators.
    pub doc: Option<String>,
    /// Whether the field renders as a commented-out entry in template output.
    /// Parsing never sets it. The template walk sets it for an absent optional
    /// field, so the template shows the field without activating it. A
    /// commented field reads as absent everywhere: the generated parse walk
    /// skips it, [`Fields::get`] and [`Fields::has`] skip it, and the layering
    /// merge drops it.
    pub commented: bool,
}

/// Whether a field was written as an attribute (`name = value`) or as a block
/// (`name { ... }` in HCL, `[name]` / `[[name]]` in TOML).
///
/// Both carry a nested structure when they name one. A block holds its
/// [`Fields`] directly. An attribute holds a [`Value`] that may be a
/// [`Map`](ValueKind::Map). The distinction is kept only so diagnostics can say
/// "found block" rather than "found object", matching how the operator wrote
/// it.
#[derive(Debug, Clone)]
pub enum FieldKind {
    /// An attribute: `name = value`.
    Value(Value),
    /// A block: `name { ... }` in HCL, `[name]` in TOML.
    Block(Fields),
}

/// One structural level: the named entries of a body, table, or inline object,
/// plus the span an enclosing-level error (a missing required field) points
/// at.
#[derive(Debug, Clone)]
pub struct Fields {
    source: SourceId,
    enclosing: Span,
    items: Vec<Field>,
}

impl Value {
    /// A value with no source location, its span the detached sentinel. Used by
    /// the `ToFields` code `#[derive(Spec)]` generates.
    pub fn detached(kind: ValueKind) -> Self {
        Self {
            span: Span::detached(),
            kind,
        }
    }

    /// A value carrying its source span. Used by the source-view walk
    /// `#[derive(Spec)]` generates, which preserves each value's location.
    pub fn spanned(span: Span, kind: ValueKind) -> Self {
        Self { span, kind }
    }
}

impl Field {
    /// An attribute field with no source location, carrying a populated value.
    /// The name, name span, and field span are all the detached sentinel.
    pub fn detached_value(name: &str, value: Value) -> Self {
        Self {
            name: name.to_string(),
            name_span: Span::detached(),
            span: Span::detached(),
            source: SourceId::DETACHED,
            kind: FieldKind::Value(value),
            doc: None,
            commented: false,
        }
    }

    /// A block field with no source location, carrying a populated nested level.
    /// The name span and field span are the detached sentinel.
    pub fn detached_block(name: &str, fields: Fields) -> Self {
        Self {
            name: name.to_string(),
            name_span: Span::detached(),
            span: Span::detached(),
            source: SourceId::DETACHED,
            kind: FieldKind::Block(fields),
            doc: None,
            commented: false,
        }
    }

    /// An attribute field carrying its source span, with the source taken from
    /// the span. The name span is the detached sentinel, because a spec field
    /// name has no source location. Used by the source-view walk
    /// `#[derive(Spec)]` generates.
    pub fn spanned_value(name: &str, span: Span, value: Value) -> Self {
        Self {
            name: name.to_string(),
            name_span: Span::detached(),
            span,
            source: span.source,
            kind: FieldKind::Value(value),
            doc: None,
            commented: false,
        }
    }

    /// A block field carrying its source span, with the source taken from the
    /// span. The name span is the detached sentinel. Used by the source-view
    /// walk `#[derive(Spec)]` generates.
    pub fn spanned_block(name: &str, span: Span, fields: Fields) -> Self {
        Self {
            name: name.to_string(),
            name_span: Span::detached(),
            span,
            source: span.source,
            kind: FieldKind::Block(fields),
            doc: None,
            commented: false,
        }
    }

    /// Attaches a doc comment, for the annotated-template walk. `None` leaves
    /// the field without a comment.
    pub fn with_doc(mut self, doc: Option<String>) -> Self {
        self.doc = doc;
        self
    }

    /// Marks the field as a commented-out entry, for the annotated-template
    /// walk.
    pub fn as_commented(mut self) -> Self {
        self.commented = true;
        self
    }
}

impl Fields {
    /// A level read from `source`, with `enclosing` as the span
    /// missing-field errors point at.
    pub fn new(source: SourceId, enclosing: Span, items: Vec<Field>) -> Self {
        Self {
            source,
            enclosing,
            items,
        }
    }

    /// A structural level with no source location, for a populated view built
    /// from a spec's defaults rather than parsed. The source and the enclosing
    /// span are the detached sentinel. Used by the `ToFields` code
    /// `#[derive(Spec)]` generates.
    pub fn detached(items: Vec<Field>) -> Self {
        Self {
            source: SourceId::DETACHED,
            enclosing: Span::detached(),
            items,
        }
    }

    /// The source this level was read from.
    pub fn source(&self) -> SourceId {
        self.source
    }

    /// The span missing-field errors point at: the surrounding block, or the
    /// whole file at the root.
    pub fn enclosing(&self) -> Span {
        self.enclosing
    }

    /// The fields in source order.
    pub fn iter(&self) -> std::slice::Iter<'_, Field> {
        self.items.iter()
    }

    #[cfg(feature = "layering")]
    pub(crate) fn into_items(self) -> Vec<Field> {
        self.items
    }

    /// Whether an active field with the name exists at this level. A commented
    /// field reads as absent, so name lookup skips it.
    pub fn has(&self, name: &str) -> bool {
        self.items
            .iter()
            .any(|field| field.name == name && !field.commented)
    }

    /// The first active field with the name, or `None`. A commented field
    /// reads as absent, so name lookup skips it. Only
    /// [`iter`](Fields::iter) exposes one, which is what the emitters and the
    /// generated walk consume.
    pub fn get(&self, name: &str) -> Option<&Field> {
        self.items
            .iter()
            .find(|field| field.name == name && !field.commented)
    }
}

/// Structural construction of `Self` from a neutral field view.
///
/// Implementations walk the fields once, match them by name, and push every
/// problem they find to the report. Returning `None` means at least one error
/// was pushed. This is the trait `#[derive(Spec)]` generates and the one a
/// handwritten spec implements. It names no format.
pub trait FromFields: Sized {
    /// Builds `Self` from one structural level, reporting every problem
    /// found. `None` means at least one error was pushed.
    fn from_fields(fields: &Fields, report: &mut Report) -> Option<Self>;
}

/// Structural emission of a neutral field view from `Self`.
///
/// This is the write-path counterpart of [`FromFields`]. Parsing reads a
/// [`Fields`] and builds a spec. Populate walks a spec and builds a [`Fields`],
/// filling every default the source omitted, so it adds to the data rather than
/// inverting the parse. `#[derive(Spec)]` generates it.
///
/// The populated and template walks detach the span of every value they
/// produce, because a filled default has no source location. The source walk,
/// [`to_source_fields`](ToFields::to_source_fields), keeps the spans the spec
/// holds, because it emits only fields a source wrote.
pub trait ToFields {
    /// The populated field model with no comments.
    fn to_fields(&self) -> Fields;

    /// The source-view field model: only the fields the source actually set,
    /// with defaults omitted. A field is included when its `Located` span is
    /// attached, and a filled default, which carries the detached sentinel, is
    /// omitted. Each included value keeps its real source span, so a
    /// location-aware consumer can find where it was written. The name span and
    /// the `Fields` container's own source and enclosing span stay detached,
    /// because a spec supplies neither.
    ///
    /// This is required, not defaulted. No correct fallback exists: the
    /// populated model would report filled defaults as operator-written, the
    /// exact confusion this view removes, so a handwritten impl must answer the
    /// question itself.
    fn to_source_fields(&self) -> Fields;

    /// The populated field model with each field's doc comment attached, for an
    /// annotated template. Defaults to [`to_fields`](ToFields::to_fields), so an
    /// impl that does not harvest comments emits none and adding the method
    /// breaks no caller.
    fn to_template(&self) -> Fields {
        self.to_fields()
    }

    /// The doc comment on the spec type itself, or `None`. A parent's template
    /// walk renders it above a block embedding this spec when the embedding
    /// field carries no doc of its own, so a spec documented once at its
    /// definition annotates every such site. Defaults to `None`, so a
    /// handwritten impl opts in rather than breaks.
    fn spec_doc(&self) -> Option<String> {
        None
    }

    /// The same doc comment as [`spec_doc`](ToFields::spec_doc), readable
    /// without an instance. The template walk uses it for the commented empty
    /// block an absent optional nested field renders, where no instance exists
    /// to ask. Defaults to `None`, so a handwritten impl opts in rather than
    /// breaks.
    fn type_doc() -> Option<String>
    where
        Self: Sized,
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_lookup_skips_a_commented_field() {
        // Arrange
        let commented = Field::detached_value(
            "pid_file",
            Value::detached(ValueKind::Scalar(Scalar::String(String::new()))),
        )
        .as_commented();
        let level = Fields::detached(vec![commented]);

        // Act
        let by_get = level.get("pid_file");
        let by_has = level.has("pid_file");

        // Assert
        // A commented field reads as absent, so only iteration exposes it.
        assert!(by_get.is_none());
        assert!(!by_has);
        assert_eq!(level.iter().count(), 1);
    }

    #[test]
    fn detached_constructors_carry_no_source_location() {
        // Arrange
        let value = Value::detached(ValueKind::Scalar(Scalar::Int(16)));
        let attribute = Field::detached_value("max_body_mb", value);
        let block = Field::detached_block("limits", Fields::detached(vec![]));
        let level = Fields::detached(vec![attribute.clone()]);

        // Act
        let attribute_detached = attribute.name_span.is_detached()
            && attribute.span.is_detached()
            && attribute.source == SourceId::DETACHED;

        // Assert
        assert!(attribute_detached);
        assert!(block.name_span.is_detached() && block.span.is_detached());
        assert!(block.source == SourceId::DETACHED);
        assert!(level.enclosing().is_detached());
        assert_eq!(level.source(), SourceId::DETACHED);
    }
}
