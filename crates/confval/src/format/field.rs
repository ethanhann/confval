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
    /// A nested structure written inline (an HCL object, a TOML inline table).
    Map(Fields),
    /// Present in source but outside the model: an HCL template or null, a
    /// TOML datetime. The label is the noun diagnostics use ("string
    /// template", "datetime"). No leaf parser matches it, so it always
    /// surfaces as a type mismatch.
    Other(&'static str),
}

/// One named entry at a structural level: an attribute, a block, or a table.
///
/// Marked non-exhaustive, so a field added here stays a minor release rather
/// than a break for a frontend outside this crate. Build one with
/// [`parsed`](Field::parsed) on the read path, or with
/// [`detached_value`](Field::detached_value) and
/// [`detached_block`](Field::detached_block) on the write path, then attach
/// what the shape needs through [`with_doc`](Field::with_doc) and
/// [`at`](Field::at).
#[derive(Debug, Clone)]
#[non_exhaustive]
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
}

/// A field at a level, and whether a template renders it active or
/// commented out.
///
/// Whether an entry is commented is a question about rendering, not about the
/// configuration, so it lives here rather than on [`Field`]. Parsing produces
/// only [`Active`](Entry::Active) entries. The template walk produces a
/// [`Commented`](Entry::Commented) entry for an absent optional field, so a
/// template shows the field without activating it.
///
/// Only the emitters read a commented entry, through
/// [`entries`](Fields::entries). Every other reader goes through
/// [`iter`](Fields::iter), [`get`](Fields::get), or [`has`](Fields::has), which
/// yield active fields alone, so nothing on the read path has to know this
/// distinction exists.
#[derive(Debug, Clone)]
pub enum Entry {
    /// A field the configuration sets.
    Active(Field),
    /// A field a template shows behind its format's comment marker.
    Commented(Field),
}

impl Entry {
    /// The field, whether it is active or commented.
    pub fn field(&self) -> &Field {
        match self {
            Entry::Active(field) | Entry::Commented(field) => field,
        }
    }

    /// Whether a template renders this entry behind a comment marker.
    pub fn is_commented(&self) -> bool {
        matches!(self, Entry::Commented(_))
    }
}

impl From<Field> for Entry {
    fn from(field: Field) -> Self {
        Entry::Active(field)
    }
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
    items: Vec<Entry>,
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
    /// A field read from a source, at the spans the frontend captured.
    ///
    /// This is the constructor a format frontend builds its fields with.
    /// `source` is the source the field was read from, `name_span` covers the
    /// name alone, where an unknown-field error points, and `span` covers the
    /// name and the value together.
    ///
    /// The doc comment belongs to the write path, so a parsed field carries
    /// none. Parsing drops a source file's comments.
    pub fn parsed(
        name: impl Into<String>,
        name_span: Span,
        span: Span,
        source: SourceId,
        kind: FieldKind,
    ) -> Self {
        Self {
            name: name.into(),
            name_span,
            span,
            source,
            kind,
            doc: None,
        }
    }

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
        }
    }

    /// Attaches a doc comment, for the annotated-template walk. `None` leaves
    /// the field without a comment.
    pub fn with_doc(mut self, doc: Option<String>) -> Self {
        self.doc = doc;
        self
    }

    /// The commented-out entry for this field, for the annotated-template walk.
    pub fn as_commented(self) -> Entry {
        Entry::Commented(self)
    }

    /// Locates a constructed field.
    ///
    /// The field's span and its source come from `span`. An attribute field's
    /// value takes the same span, because a spec field has one location that
    /// both halves of it report. A block field's nested level is left alone,
    /// because its source and enclosing span belong to the level rather than to
    /// the field naming it.
    ///
    /// A sequence value takes the span as the whole list's location. Its
    /// elements keep whatever spans they were built with.
    pub fn at(mut self, span: Span) -> Self {
        self.span = span;
        self.source = span.source;
        if let FieldKind::Value(value) = &mut self.kind {
            value.span = span;
        }
        self
    }
}

impl Fields {
    /// A level read from `source`, with `enclosing` as the span
    /// missing-field errors point at.
    pub fn new(source: SourceId, enclosing: Span, items: Vec<Field>) -> Self {
        Self::from_entries(
            source,
            enclosing,
            items.into_iter().map(Entry::Active).collect(),
        )
    }

    /// A level built from entries, so a template can carry commented ones.
    pub fn from_entries(source: SourceId, enclosing: Span, items: Vec<Entry>) -> Self {
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
        Self::detached_entries(items.into_iter().map(Entry::Active).collect())
    }

    /// A detached level built from entries, the shape the template walk
    /// produces.
    pub fn detached_entries(items: Vec<Entry>) -> Self {
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

    /// The active fields in source order.
    ///
    /// A commented entry is a template's rendering of a field the
    /// configuration does not set, so it is not a field this level has. Only
    /// [`entries`](Fields::entries) exposes one.
    pub fn iter(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|entry| match entry {
            Entry::Active(field) => Some(field),
            Entry::Commented(_) => None,
        })
    }

    /// Every entry in source order, commented ones included. This is the
    /// emitters' view, and the only one that sees a commented entry.
    pub fn entries(&self) -> std::slice::Iter<'_, Entry> {
        self.items.iter()
    }

    #[cfg(feature = "layering")]
    pub(crate) fn into_items(self) -> Vec<Field> {
        self.items
            .into_iter()
            .filter_map(|entry| match entry {
                Entry::Active(field) => Some(field),
                Entry::Commented(_) => None,
            })
            .collect()
    }

    /// Whether a field with the name exists at this level.
    pub fn has(&self, name: &str) -> bool {
        self.iter().any(|field| field.name == name)
    }

    /// The first field with the name, or `None`.
    pub fn get(&self, name: &str) -> Option<&Field> {
        self.iter().find(|field| field.name == name)
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

    /// The source-view field model. It holds only the fields the source set,
    /// with defaults omitted. A field is included when its `Located` span is
    /// attached, and a filled default, which carries the detached sentinel, is
    /// omitted. Each included value keeps its real source span, so a
    /// location-aware consumer can find where it was written. The name span and
    /// the `Fields` container's own source and enclosing span stay detached,
    /// because a spec supplies neither.
    ///
    /// The method is required. No correct fallback exists. A defaulted
    /// implementation would return the populated model, which reports filled
    /// defaults as operator-written. That is the confusion this view removes, so
    /// a handwritten impl must answer the question itself.
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
        let level = Fields::detached_entries(vec![commented]);

        // Act
        let by_get = level.get("pid_file");
        let by_has = level.has("pid_file");

        // Assert
        // A commented entry reads as absent, so only `entries` exposes it.
        assert!(by_get.is_none());
        assert!(!by_has);
        assert_eq!(level.iter().count(), 0);
        assert_eq!(level.entries().count(), 1);
    }

    #[test]
    fn name_lookup_answers_each_name_at_a_mixed_level() {
        // Arrange
        // Three shapes at one level: an active field, a commented entry with a
        // name nothing else uses, and a commented entry shadowing the active
        // name. A lookup that ignored the name, or that inverted the match,
        // would answer at least one of these wrongly.
        let int = |value: i64| Value::detached(ValueKind::Scalar(Scalar::Int(value)));
        let active = Field::detached_value("port", int(8080));
        let commented_only = Field::detached_value("pid_file", int(0)).as_commented();
        let shadowing = Field::detached_value("port", int(1)).as_commented();
        let level = Fields::detached_entries(vec![active.into(), commented_only, shadowing]);

        // Act
        let answers = [
            level.has("port"),
            level.has("pid_file"),
            level.has("hostname"),
        ];

        // Assert
        assert_eq!(answers, [true, false, false]);
        // `get` answers the same way and hands back the active field, not the
        // commented entry that shares its name.
        assert!(level.get("pid_file").is_none());
        assert!(level.get("hostname").is_none());
        let found = level.get("port").expect("the active field is present");
        let FieldKind::Value(Value {
            kind: ValueKind::Scalar(Scalar::Int(port)),
            ..
        }) = &found.kind
        else {
            panic!("port should be an integer attribute");
        };
        assert_eq!(*port, 8080);
    }

    #[test]
    fn the_spec_doc_defaults_answer_none_for_a_handwritten_impl() {
        // Arrange
        // `#[derive(Spec)]` overrides both methods whenever the struct carries
        // a doc comment, so the defaults are reached only by a handwritten
        // impl. One that declares no documentation must not acquire any, or a
        // parent's template walk would render a comment nobody wrote.
        struct Handwritten;

        impl ToFields for Handwritten {
            fn to_fields(&self) -> Fields {
                Fields::detached(Vec::new())
            }

            fn to_source_fields(&self) -> Fields {
                Fields::detached(Vec::new())
            }
        }

        // Act
        let from_instance = Handwritten.spec_doc();
        let from_type = Handwritten::type_doc();

        // Assert
        assert_eq!(from_instance, None);
        assert_eq!(from_type, None);
        // The template walk defaults to the plain one, so an impl that
        // harvests no comments emits none either.
        assert_eq!(Handwritten.to_template().entries().count(), 0);
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
