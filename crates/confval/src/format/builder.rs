//! Building a `Fields` by hand, one field at a time.
//!
//! `#[derive(Spec)]` generates both write walks. The shapes it cannot express
//! are written with a handwritten [`FromFields`](super::FromFields). Such a
//! spec implements [`ToFields`] by hand, which means reproducing two walks that
//! differ per field. The populated walk emits every field and
//! detaches every span. The source walk omits a field whose span is detached,
//! keeps the span of the fields it emits, and recurses into children with
//! `to_source_fields` rather than `to_fields`.
//!
//! [`FieldsBuilder`] takes the walk as a parameter, so an impl lists its fields
//! once and both walks read that one list. Each method takes the `Located`
//! rather than the value inside it, so the builder holds the span and the
//! attachment each walk needs. The recursion into children follows the same
//! walk.

use super::field::{Field, Fields, Scalar, ToFields, Value, ValueKind};
use crate::source::{Located, Span};
use std::path::PathBuf;

/// Which walk a [`FieldsBuilder`] is building.
///
/// Marked non-exhaustive, so a third walk stays a minor release rather than a
/// break for a downstream match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Walk {
    /// The `to_fields` walk. Every field is emitted and every span is detached.
    Populated,
    /// The `to_source_fields` walk: only the fields a source set, each keeping
    /// its span.
    Source,
}

mod sealed {
    use super::Scalar;

    /// Carries the conversion so the public [`Leaf`](super::Leaf) trait has no
    /// callable method. Without this split, every `String` and `i64` in a scope
    /// that imports `Leaf` would gain a `.scalar()` only the builder calls.
    pub trait Sealed {
        fn scalar(&self) -> Scalar;
    }
}

/// The scalar types a spec field holds.
///
/// `String`, `i64`, `f64`, `bool`, and `PathBuf` implement it. The list mirrors
/// the leaf types `#[derive(Spec)]` accepts, so a handwritten impl and a derived
/// one cannot disagree about what a leaf is, and it can grow in a minor release.
///
/// The trait is sealed. No crate outside confval can implement it, so a leaf
/// type added here breaks no code elsewhere.
///
/// `PathBuf` emits as a string through `to_string_lossy`, the one lossy leaf,
/// matching what the derive generates for a `Located<PathBuf>` field.
pub trait Leaf: sealed::Sealed {}

impl<T: sealed::Sealed> Leaf for T {}

impl sealed::Sealed for String {
    fn scalar(&self) -> Scalar {
        Scalar::String(self.clone())
    }
}

impl sealed::Sealed for i64 {
    fn scalar(&self) -> Scalar {
        Scalar::Int(*self)
    }
}

impl sealed::Sealed for f64 {
    fn scalar(&self) -> Scalar {
        Scalar::Float(*self)
    }
}

impl sealed::Sealed for bool {
    fn scalar(&self) -> Scalar {
        Scalar::Bool(*self)
    }
}

impl sealed::Sealed for PathBuf {
    fn scalar(&self) -> Scalar {
        Scalar::String(self.to_string_lossy().into_owned())
    }
}

/// One structural level under construction, for a handwritten [`ToFields`].
///
/// Build it with the walk you are implementing, add one call per field, and
/// finish. The walk decides what each method does with each span it is given.
/// The populated walk emits everything detached. The source walk emits what a
/// source set and keeps its location.
///
/// ```rust
/// use confval::format::{Fields, FieldsBuilder, ToFields, Walk};
/// use confval::prelude::*;
///
/// struct Server {
///     hostname: Located<String>,
///     port: Option<Located<i64>>,
/// }
///
/// impl Server {
///     fn build(&self, walk: Walk) -> Fields {
///         FieldsBuilder::new(walk)
///             .leaf("hostname", &self.hostname)
///             .leaf_opt("port", self.port.as_ref())
///             .finish()
///     }
/// }
///
/// impl ToFields for Server {
///     fn to_fields(&self) -> Fields {
///         self.build(Walk::Populated)
///     }
///
///     fn to_source_fields(&self) -> Fields {
///         self.build(Walk::Source)
///     }
/// }
/// ```
pub struct FieldsBuilder {
    walk: Walk,
    items: Vec<Field>,
}

impl FieldsBuilder {
    /// A builder for the given walk.
    pub fn new(walk: Walk) -> Self {
        Self {
            walk,
            items: Vec::new(),
        }
    }

    /// A required leaf. The populated walk emits it detached. The source walk
    /// emits it with its span, and omits it when the span is detached.
    pub fn leaf<T: Leaf>(self, name: &str, value: &Located<T>) -> Self {
        self.push_scalar(name, value.span, sealed::Sealed::scalar(&value.value))
    }

    /// An optional leaf. An absent field is omitted by both walks.
    pub fn leaf_opt<T: Leaf>(self, name: &str, value: Option<&Located<T>>) -> Self {
        match value {
            Some(value) => self.leaf(name, value),
            None => self,
        }
    }

    /// A required string list, the bare `Vec<Located<String>>` shape, which
    /// holds no span of its own.
    ///
    /// The populated walk emits every element detached. The source walk emits
    /// the elements whose span is attached, each with its span, and omits the
    /// field when none survive. A list the source wrote empty is therefore
    /// indistinguishable from an absent one.
    pub fn string_list(mut self, name: &str, values: &[Located<String>]) -> Self {
        let elements: Vec<Value> = match self.walk {
            Walk::Populated => values.iter().map(detached_element).collect(),
            Walk::Source => values
                .iter()
                .filter(|element| !element.span.is_detached())
                .map(spanned_element)
                .collect(),
        };
        if self.walk == Walk::Populated || !elements.is_empty() {
            self.items.push(seq_field(name, elements));
        }
        self
    }

    /// An optional string list, the wrapped
    /// `Option<Located<Vec<Located<String>>>>` shape, whose wrapper keeps the
    /// list's own span.
    ///
    /// The populated walk emits it detached when present. The source walk emits
    /// it when the wrapper span is attached, with that span and each
    /// element's own, so a list the source wrote empty survives.
    pub fn string_list_opt(
        mut self,
        name: &str,
        value: Option<&Located<Vec<Located<String>>>>,
    ) -> Self {
        let Some(list) = value else {
            return self;
        };
        match self.walk {
            Walk::Populated => {
                let elements = list.value.iter().map(detached_element).collect();
                self.items.push(seq_field(name, elements));
            }
            Walk::Source => {
                if !list.span.is_detached() {
                    let elements = list.value.iter().map(spanned_element).collect();
                    self.items.push(seq_field(name, elements).at(list.span));
                }
            }
        }
        self
    }

    /// A required nested block. The populated walk recurses with `to_fields`.
    /// The source walk recurses with `to_source_fields` and keeps the block's
    /// span, and omits the block when that span is detached.
    pub fn block<S: ToFields>(mut self, name: &str, value: &Located<S>) -> Self {
        match self.walk {
            Walk::Populated => {
                self.items
                    .push(Field::detached_block(name, value.value.to_fields()));
            }
            Walk::Source => {
                if !value.span.is_detached() {
                    self.items.push(
                        Field::detached_block(name, value.value.to_source_fields()).at(value.span),
                    );
                }
            }
        }
        self
    }

    /// An optional nested block. An absent block is omitted by both walks.
    pub fn block_opt<S: ToFields>(self, name: &str, value: Option<&Located<S>>) -> Self {
        match value {
            Some(value) => self.block(name, value),
            None => self,
        }
    }

    /// An optional nested block whose absence the populated walk fills from
    /// `S::default()`, the counterpart of `#[confval(nested, default)]`.
    ///
    /// The populated walk emits the block either way, from the value when it is
    /// present and from `S::default()` when it is not, so an operator reading
    /// that view sees the values the program will run with. The source walk
    /// omits an absent block, because a default is not something the source set.
    ///
    /// Use [`block_opt`](Self::block_opt) for an optional block with no default,
    /// which both walks omit when it is absent.
    pub fn block_opt_default<S: ToFields + Default>(
        mut self,
        name: &str,
        value: Option<&Located<S>>,
    ) -> Self {
        match (self.walk, value) {
            (_, Some(value)) => self.block(name, value),
            (Walk::Populated, None) => {
                let filled = S::default();
                self.items
                    .push(Field::detached_block(name, filled.to_fields()));
                self
            }
            (Walk::Source, None) => self,
        }
    }

    /// A repeated nested block. Each element is emitted the way [`block`](Self::block)
    /// emits one, so the source walk skips an element whose span is detached.
    pub fn block_list<S: ToFields>(mut self, name: &str, values: &[Located<S>]) -> Self {
        for value in values {
            self = self.block(name, value);
        }
        self
    }

    /// A field whose name and value the impl supplies itself, with no `Located`
    /// behind it. This is the tagged enum's discriminator, the `type` or `mode`
    /// field that says which variant follows.
    ///
    /// Both walks emit it. A source view that dropped the tag would not
    /// reparse, so the tag is part of what the source set even when the spec
    /// type kept no span for it.
    pub fn literal_string(mut self, name: &str, value: &str) -> Self {
        self.items.push(Field::detached_value(
            name,
            Value::detached(ValueKind::Scalar(Scalar::String(value.to_string()))),
        ));
        self
    }

    /// Adds a field the builder does not shape, in the order it is called.
    ///
    /// A string-keyed map is one such shape. The walk does not reach a pushed
    /// field. The caller decides what it holds. On a source walk that includes
    /// deciding whether the source set it. Build the field with
    /// [`Field::detached_value`](Field::detached_value) or
    /// [`Field::detached_block`](Field::detached_block), and locate it with
    /// [`at`](Field::at) when it has a span.
    pub fn push(mut self, field: Field) -> Self {
        self.items.push(field);
        self
    }

    /// The finished level. The container itself is detached, because a spec
    /// supplies neither a source nor an enclosing span.
    ///
    /// This consumes the builder, so a second call is a compile error rather
    /// than an empty level.
    pub fn finish(self) -> Fields {
        Fields::detached(self.items)
    }

    fn push_scalar(mut self, name: &str, span: Span, scalar: Scalar) -> Self {
        let kind = ValueKind::Scalar(scalar);
        match self.walk {
            Walk::Populated => {
                self.items
                    .push(Field::detached_value(name, Value::detached(kind)));
            }
            Walk::Source => {
                if !span.is_detached() {
                    self.items
                        .push(Field::detached_value(name, Value::detached(kind)).at(span));
                }
            }
        }
        self
    }
}

fn seq_field(name: &str, elements: Vec<Value>) -> Field {
    Field::detached_value(name, Value::detached(ValueKind::Seq(elements)))
}

fn detached_element(element: &Located<String>) -> Value {
    Value::detached(ValueKind::Scalar(Scalar::String(element.value.clone())))
}

fn spanned_element(element: &Located<String>) -> Value {
    Value::spanned(
        element.span,
        ValueKind::Scalar(Scalar::String(element.value.clone())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::field::FieldKind;
    use crate::source::SourceId;

    fn span_at(start: u32, end: u32) -> Span {
        Span::new(SourceId(7), start, end)
    }

    /// A child with one field, so a recursion that reads the wrong walk leaves
    /// `size` present or absent.
    struct Child {
        size: Located<i64>,
    }

    /// The fill `block_opt_default` uses for an absent block, matching what a
    /// `#[confval(derive_default)]` child would supply.
    impl Default for Child {
        fn default() -> Self {
            Child {
                size: Located::detached(16),
            }
        }
    }

    impl ToFields for Child {
        fn to_fields(&self) -> Fields {
            FieldsBuilder::new(Walk::Populated)
                .leaf("size", &self.size)
                .finish()
        }

        fn to_source_fields(&self) -> Fields {
            FieldsBuilder::new(Walk::Source)
                .leaf("size", &self.size)
                .finish()
        }
    }

    fn names(fields: &Fields) -> Vec<String> {
        fields.iter().map(|field| field.name.clone()).collect()
    }

    fn only(fields: &Fields) -> Field {
        fields.iter().next().expect("one field").clone()
    }

    #[test]
    fn the_populated_walk_emits_every_leaf_detached() {
        // Arrange
        let value = Located::new("h".to_string(), span_at(0, 3));

        // Act
        let fields = FieldsBuilder::new(Walk::Populated)
            .leaf("hostname", &value)
            .finish();

        // Assert
        let field = only(&fields);
        assert_eq!(field.name, "hostname");
        assert!(field.span.is_detached());
        assert_eq!(field.source, SourceId::DETACHED);
        match &field.kind {
            FieldKind::Value(value) => {
                assert!(value.span.is_detached());
                assert!(
                    matches!(&value.kind, ValueKind::Scalar(Scalar::String(text)) if text == "h")
                );
            }
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn the_source_walk_keeps_an_attached_leaf_and_its_span() {
        // Arrange
        let value = Located::new(8080_i64, span_at(4, 8));

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .leaf("port", &value)
            .finish();

        // Assert
        let field = only(&fields);
        assert_eq!(field.span, span_at(4, 8));
        assert_eq!(field.source, SourceId(7));
        assert!(field.name_span.is_detached());
        match &field.kind {
            FieldKind::Value(value) => assert_eq!(value.span, span_at(4, 8)),
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn the_source_walk_omits_a_detached_leaf() {
        // Arrange
        let value = Located::detached(4_i64);

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .leaf("workers", &value)
            .finish();

        // Assert
        assert!(names(&fields).is_empty());
    }

    #[test]
    fn every_leaf_type_emits_its_scalar() {
        // Arrange
        let path = Located::detached(PathBuf::from("/tmp/pid"));

        // Act
        let fields = FieldsBuilder::new(Walk::Populated)
            .leaf("s", &Located::detached("x".to_string()))
            .leaf("i", &Located::detached(1_i64))
            .leaf("f", &Located::detached(2.5_f64))
            .leaf("b", &Located::detached(true))
            .leaf("p", &path)
            .finish();

        // Assert
        let scalars: Vec<Scalar> = fields
            .iter()
            .map(|field| match &field.kind {
                FieldKind::Value(value) => match &value.kind {
                    ValueKind::Scalar(scalar) => scalar.clone(),
                    other => panic!("expected a scalar, got {other:?}"),
                },
                other => panic!("expected a value, got {other:?}"),
            })
            .collect();
        assert_eq!(
            scalars,
            vec![
                Scalar::String("x".to_string()),
                Scalar::Int(1),
                Scalar::Float(2.5),
                Scalar::Bool(true),
                Scalar::String("/tmp/pid".to_string()),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_with_a_non_utf8_byte_emits_lossily() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // Arrange
        let raw = OsStr::from_bytes(&[b'/', b't', b'm', b'p', b'/', 0xff]);
        let path = Located::detached(PathBuf::from(raw));

        // Act
        let fields = FieldsBuilder::new(Walk::Populated)
            .leaf("p", &path)
            .finish();

        // Assert
        match &only(&fields).kind {
            FieldKind::Value(value) => assert!(
                matches!(&value.kind, ValueKind::Scalar(Scalar::String(text)) if text == "/tmp/\u{fffd}")
            ),
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn an_absent_optional_leaf_is_omitted_by_both_walks() {
        // Arrange
        let absent: Option<Located<i64>> = None;

        // Act
        let populated = FieldsBuilder::new(Walk::Populated)
            .leaf_opt("port", absent.as_ref())
            .finish();
        let source = FieldsBuilder::new(Walk::Source)
            .leaf_opt("port", absent.as_ref())
            .finish();

        // Assert
        assert!(names(&populated).is_empty());
        assert!(names(&source).is_empty());
    }

    #[test]
    fn the_populated_walk_emits_a_bare_list_with_detached_elements() {
        // Arrange
        let values = vec![Located::new("a".to_string(), span_at(0, 3))];

        // Act
        let fields = FieldsBuilder::new(Walk::Populated)
            .string_list("tags", &values)
            .finish();

        // Assert
        let field = only(&fields);
        assert!(field.span.is_detached());
        match &field.kind {
            FieldKind::Value(value) => match &value.kind {
                ValueKind::Seq(elements) => {
                    assert_eq!(elements.len(), 1);
                    assert!(elements[0].span.is_detached());
                }
                other => panic!("expected a sequence, got {other:?}"),
            },
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn the_source_walk_gives_a_bare_list_a_detached_field_and_spanned_elements() {
        // Arrange
        // The bare list has no wrapper span, so its elements keep the only
        // locations it has, and a detached element was never written.
        let values = vec![
            Located::new("a".to_string(), span_at(0, 3)),
            Located::detached("b".to_string()),
        ];

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .string_list("tags", &values)
            .finish();

        // Assert
        let field = only(&fields);
        assert!(field.span.is_detached(), "the field has no location");
        match &field.kind {
            FieldKind::Value(value) => {
                assert!(value.span.is_detached(), "the wrapper has no location");
                match &value.kind {
                    ValueKind::Seq(elements) => {
                        assert_eq!(elements.len(), 1, "the detached element is dropped");
                        assert_eq!(elements[0].span, span_at(0, 3));
                    }
                    other => panic!("expected a sequence, got {other:?}"),
                }
            }
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn the_source_walk_omits_a_bare_list_with_no_attached_element() {
        // Arrange
        let values = vec![Located::detached("a".to_string())];

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .string_list("tags", &values)
            .finish();

        // Assert
        assert!(names(&fields).is_empty());
    }

    #[test]
    fn the_populated_walk_emits_an_empty_bare_list() {
        // Arrange
        let values: Vec<Located<String>> = Vec::new();

        // Act
        let fields = FieldsBuilder::new(Walk::Populated)
            .string_list("tags", &values)
            .finish();

        // Assert
        assert_eq!(names(&fields), vec!["tags"]);
    }

    #[test]
    fn a_wrapped_empty_list_survives_the_source_walk() {
        // Arrange
        // The wrapper keeps the list's own span, so an empty list the source
        // wrote is distinguishable from an absent one.
        let list = Located::new(Vec::new(), span_at(9, 11));

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .string_list_opt("allow", Some(&list))
            .finish();

        // Assert
        let field = only(&fields);
        assert_eq!(field.span, span_at(9, 11));
        match &field.kind {
            FieldKind::Value(value) => {
                assert_eq!(value.span, span_at(9, 11));
                assert!(matches!(&value.kind, ValueKind::Seq(elements) if elements.is_empty()));
            }
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn the_source_walk_omits_a_wrapped_list_with_a_detached_wrapper() {
        // Arrange
        let list = Located::detached(vec![Located::new("a".to_string(), span_at(0, 3))]);

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .string_list_opt("allow", Some(&list))
            .finish();

        // Assert
        assert!(names(&fields).is_empty());
    }

    #[test]
    fn a_block_recurses_with_the_walk_it_was_built_for() {
        // Arrange
        // The child's one field is detached, so the source walk drops it and the
        // populated walk keeps it. A recursion that read the wrong walk would
        // leave the child's field in the source view.
        let child = Located::new(
            Child {
                size: Located::detached(16),
            },
            span_at(0, 12),
        );

        // Act
        let populated = FieldsBuilder::new(Walk::Populated)
            .block("limits", &child)
            .finish();
        let source = FieldsBuilder::new(Walk::Source)
            .block("limits", &child)
            .finish();

        // Assert
        assert_eq!(child_names(&populated), vec!["size"]);
        assert!(child_names(&source).is_empty());
    }

    fn child_names(fields: &Fields) -> Vec<String> {
        match &only(fields).kind {
            FieldKind::Block(inner) => names(inner),
            other => panic!("expected a block, got {other:?}"),
        }
    }

    #[test]
    fn the_source_walk_omits_a_detached_block() {
        // Arrange
        let child = Located::detached(Child {
            size: Located::new(16, span_at(0, 2)),
        });

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .block("limits", &child)
            .finish();

        // Assert
        assert!(names(&fields).is_empty());
    }

    #[test]
    fn an_absent_optional_block_is_omitted_by_both_walks() {
        // Arrange
        let absent: Option<Located<Child>> = None;

        // Act
        let populated = FieldsBuilder::new(Walk::Populated)
            .block_opt("limits", absent.as_ref())
            .finish();
        let source = FieldsBuilder::new(Walk::Source)
            .block_opt("limits", absent.as_ref())
            .finish();

        // Assert
        assert!(names(&populated).is_empty());
        assert!(names(&source).is_empty());
    }

    #[test]
    fn the_populated_walk_fills_an_absent_defaulted_block() {
        // Arrange
        // This is the shape `#[confval(nested, default)]` marks, whose populated
        // walk shows the values the program will run with.
        let absent: Option<Located<Child>> = None;

        // Act
        let fields = FieldsBuilder::new(Walk::Populated)
            .block_opt_default("limits", absent.as_ref())
            .finish();

        // Assert
        assert_eq!(child_names(&fields), vec!["size"]);
    }

    #[test]
    fn the_source_walk_omits_an_absent_defaulted_block() {
        // Arrange
        let absent: Option<Located<Child>> = None;

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .block_opt_default("limits", absent.as_ref())
            .finish();

        // Assert
        assert!(
            names(&fields).is_empty(),
            "a default is not what a source set"
        );
    }

    #[test]
    fn a_present_defaulted_block_emits_its_own_value() {
        // Arrange
        let present = Located::new(
            Child {
                size: Located::new(3, span_at(0, 1)),
            },
            span_at(0, 12),
        );

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .block_opt_default("limits", Some(&present))
            .finish();

        // Assert
        assert_eq!(only(&fields).span, span_at(0, 12));
        assert_eq!(child_names(&fields), vec!["size"]);
    }

    #[test]
    fn the_populated_walk_emits_a_wrapped_list_detached() {
        // Arrange
        let list = Located::new(
            vec![Located::new("a".to_string(), span_at(1, 4))],
            span_at(0, 6),
        );

        // Act
        let fields = FieldsBuilder::new(Walk::Populated)
            .string_list_opt("allow", Some(&list))
            .finish();

        // Assert
        let field = only(&fields);
        assert!(field.span.is_detached());
        match &field.kind {
            FieldKind::Value(value) => {
                assert!(value.span.is_detached());
                match &value.kind {
                    ValueKind::Seq(elements) => assert!(elements[0].span.is_detached()),
                    other => panic!("expected a sequence, got {other:?}"),
                }
            }
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn an_absent_wrapped_list_is_omitted_by_both_walks() {
        // Arrange
        let absent: Option<Located<Vec<Located<String>>>> = None;

        // Act
        let populated = FieldsBuilder::new(Walk::Populated)
            .string_list_opt("allow", absent.as_ref())
            .finish();
        let source = FieldsBuilder::new(Walk::Source)
            .string_list_opt("allow", absent.as_ref())
            .finish();

        // Assert
        assert!(names(&populated).is_empty());
        assert!(names(&source).is_empty());
    }

    #[test]
    fn a_block_list_emits_one_field_per_element() {
        // Arrange
        let values = vec![
            Located::new(
                Child {
                    size: Located::detached(1),
                },
                span_at(0, 4),
            ),
            Located::detached(Child {
                size: Located::detached(2),
            }),
        ];

        // Act
        let populated = FieldsBuilder::new(Walk::Populated)
            .block_list("services", &values)
            .finish();
        let source = FieldsBuilder::new(Walk::Source)
            .block_list("services", &values)
            .finish();

        // Assert
        assert_eq!(names(&populated), vec!["services", "services"]);
        assert_eq!(
            names(&source),
            vec!["services"],
            "the detached element is skipped"
        );
    }

    #[test]
    fn a_literal_field_is_emitted_by_both_walks() {
        // Arrange
        let walks = [Walk::Populated, Walk::Source];

        // Act
        let emitted: Vec<Vec<String>> = walks
            .iter()
            .map(|walk| {
                names(
                    &FieldsBuilder::new(*walk)
                        .literal_string("mode", "acme")
                        .finish(),
                )
            })
            .collect();

        // Assert
        assert_eq!(emitted, vec![vec!["mode"], vec!["mode"]]);
    }

    #[test]
    fn a_pushed_field_lands_unchanged_in_call_order() {
        // Arrange
        // A map is the shape the builder does not name, so the caller builds the
        // field and decides what it holds.
        let map = Fields::detached(vec![Field::detached_value(
            "a",
            Value::detached(ValueKind::Scalar(Scalar::String("b".to_string()))),
        )]);
        let field = Field::detached_value("config", Value::detached(ValueKind::Map(map)));

        // Act
        let fields = FieldsBuilder::new(Walk::Source)
            .leaf("name", &Located::new("h".to_string(), span_at(0, 3)))
            .push(field)
            .finish();

        // Assert
        assert_eq!(names(&fields), vec!["name", "config"]);
        let pushed = fields.get("config").expect("config is present").clone();
        assert!(pushed.span.is_detached(), "the walk does not reach it");
        assert!(matches!(
            &pushed.kind,
            FieldKind::Value(value) if matches!(value.kind, ValueKind::Map(_))
        ));
    }

    #[test]
    fn at_locates_a_value_field_and_its_value() {
        // Arrange
        let field = Field::detached_value(
            "port",
            Value::detached(ValueKind::Scalar(Scalar::Int(8080))),
        );

        // Act
        let located = field.at(span_at(4, 8));

        // Assert
        assert_eq!(located.span, span_at(4, 8));
        assert_eq!(located.source, SourceId(7));
        assert!(located.name_span.is_detached());
        match &located.kind {
            FieldKind::Value(value) => assert_eq!(value.span, span_at(4, 8)),
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn at_locates_a_block_field_and_leaves_its_level_alone() {
        // Arrange
        let field = Field::detached_block("limits", Fields::detached(Vec::new()));

        // Act
        let located = field.at(span_at(0, 12));

        // Assert
        assert_eq!(located.span, span_at(0, 12));
        assert_eq!(located.source, SourceId(7));
        match &located.kind {
            FieldKind::Block(inner) => {
                assert_eq!(inner.source(), SourceId::DETACHED);
                assert!(inner.enclosing().is_detached());
            }
            other => panic!("expected a block, got {other:?}"),
        }
    }

    #[test]
    fn at_leaves_sequence_elements_alone() {
        // Arrange
        let elements = vec![Value::spanned(
            span_at(1, 4),
            ValueKind::Scalar(Scalar::String("a".to_string())),
        )];

        // Act
        let located = seq_field("allow", elements).at(span_at(0, 6));

        // Assert
        match &located.kind {
            FieldKind::Value(value) => {
                assert_eq!(value.span, span_at(0, 6), "the list takes the field's span");
                match &value.kind {
                    ValueKind::Seq(elements) => assert_eq!(elements[0].span, span_at(1, 4)),
                    other => panic!("expected a sequence, got {other:?}"),
                }
            }
            other => panic!("expected a value, got {other:?}"),
        }
    }
}
