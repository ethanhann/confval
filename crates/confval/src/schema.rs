//! The type-level walk over a spec: the schema IR.
//!
//! The two value walks answer a question about a value. [`FromFields`] reads a
//! neutral [`Fields`] and builds a spec, and [`ToFields`] walks a spec instance
//! and builds a `Fields`, filling every default the source left out. Both need
//! an instance, because both read a value.
//!
//! An editor asks a different question. Before an operator writes a value, the
//! editor needs the type: which fields are legal here, which are required, what
//! kind each one holds, and which values a closed-set field accepts. This module
//! answers that. [`ToSchema::schema`] returns a [`Schema`], a type-level
//! description of a spec derived from the struct rather than from an instance,
//! so it is an associated function with no `self`.
//!
//! `#[derive(Spec)]` emits an `impl ToSchema` for every spec, the way it emits
//! `impl ToFields`. A handwritten spec implements it too, building its tree
//! through [`Schema::new`] and [`SchemaField::new`] rather than a struct literal,
//! because every node type is `#[non_exhaustive]`.
//!
//! [`FromFields`]: crate::format::FromFields
//! [`Fields`]: crate::format::Fields
//! [`ToFields`]: crate::format::ToFields

/// The type-level walk. `#[derive(Spec)]` implements it, and a handwritten spec
/// can too. The method is associated, not a `&self` method, because the IR
/// describes a type and needs no instance.
///
/// The walk is eager. A `Block` field builds its child's schema at once, so
/// `schema()` requires a spec whose block nesting terminates. A spec that nests
/// itself, directly or through a chain, such as a field of `Vec<Located<Self>>`,
/// recurses until it overflows the stack. The value walks bound their recursion
/// by the data they read, so this limit belongs to the schema walk alone.
pub trait ToSchema {
    /// The schema of this spec level.
    fn schema() -> Schema;
}

/// One structural level of a spec, described at the type level.
///
/// Build it with [`Schema::new`]. The struct is `#[non_exhaustive]`, so a later
/// release can add a field without a break, and a producer outside this crate
/// cannot use a struct literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Schema {
    /// The spec type's own doc comment, or `None`.
    pub doc: Option<String>,
    /// The fields at this level, in declaration order.
    pub fields: Vec<SchemaField>,
}

/// One field at a level.
///
/// Build it with [`SchemaField::new`]. The struct is `#[non_exhaustive]` for the
/// same reason [`Schema`] is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct SchemaField {
    /// The field name as it appears in a config file.
    pub name: String,
    /// The field's doc comment, or `None`. This is the field's own `///` doc,
    /// not the child struct's doc that the template walk folds in for a docless
    /// block. A block's own documentation is the child level's [`Schema::doc`].
    pub doc: Option<String>,
    /// Whether an absent field is a parse error. It is `structurally_required
    /// && !has_default`: false for an `Option` field, false for a zero-or-more
    /// block list, and false for any field with a `#[confval(default)]`.
    pub required: bool,
    /// Whether the field declares a `#[confval(default)]`. For an optional
    /// nested block this records the `#[confval(nested, default)]` populate
    /// marker. There the spec field stays absent and the config side fills the
    /// default, so a hover should not read it as filled when absent.
    pub has_default: bool,
    /// The default value rendered to text, for a scalar leaf that declares one,
    /// or `None`. The derive evaluates the default expression when `schema()`
    /// runs and renders it per leaf: a string as its text, an integer and a
    /// boolean through their display forms, a float in the form the emitters
    /// write so a whole number keeps its `.0`, and a path through its lossy
    /// string form. A defaulted list, map, or block carries `None`, because
    /// there is no single value to render. The reader pairs the text with the
    /// field's leaf type to know what it holds.
    pub default_text: Option<String>,
    /// The field's declared type.
    pub ty: SchemaType,
    /// Whether this field is its block's label field, marked `#[confval(label)]`.
    /// The reference pass reads a block instance's label from the field flagged
    /// here when the native label slot is empty.
    pub label: bool,
}

/// A field's declared type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SchemaType {
    /// A single scalar leaf, with the constraint it declares, if any.
    Scalar {
        /// The leaf's config-level type.
        leaf: ScalarType,
        /// The mechanical constraint the field records, or `None`.
        constraint: Option<Constraint>,
    },
    /// A list of strings.
    StringList,
    /// A nested block. `repeated` is true for a zero-or-more block list.
    Block {
        /// The child level's schema.
        schema: Box<Schema>,
        /// Whether the block is a zero-or-more list rather than a single block.
        repeated: bool,
    },
    /// An open-ended, string-keyed map with string values. Keys are open, so the
    /// node names no key or value type, mirroring [`StringList`](SchemaType::StringList).
    StringMap,
}

/// The scalar leaf types the derive can classify. `Path` is the config-level
/// name for a `PathBuf` field, because the IR names the string an operator
/// writes rather than the Rust wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScalarType {
    /// A `String` leaf.
    String,
    /// An `i64` leaf.
    Int,
    /// An `f64` leaf.
    Float,
    /// A `bool` leaf.
    Bool,
    /// A `PathBuf` leaf, named `Path` for the config-level string an operator
    /// writes.
    Path,
}

/// A mechanical constraint the derive can read from a recording attribute.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Constraint {
    /// The allowed strings of a `keyword_enum!`, in declaration order.
    Keywords(&'static [&'static str]),
    /// An inclusive numeric range, with its bounds rendered to text for a
    /// text-facing hover or diagnostic line. `help` carries the constraint's
    /// custom help line for the hover, or `None`.
    Range {
        /// The smallest allowed value, rendered to text.
        min: String,
        /// The largest allowed value, rendered to text.
        max: String,
        /// A unit suffix for the hover, such as "seconds", or `None`.
        units: Option<&'static str>,
        /// The constraint's custom help line for the hover, or `None`.
        help: Option<&'static str>,
    },
    /// The value references the labels of a block its scope can see. The block
    /// is named by its config field name, the `<block>` of
    /// `#[confval(references = <block>)]`. The reference pass resolves the name
    /// outward from the reference's enclosing block to the nearest enclosing
    /// scope that declares a labeled block field of that name. The root is
    /// searched last. The value is checked against that scope's labels.
    References {
        /// The config field name of the referenced labeled block.
        block: &'static str,
    },
}

impl Schema {
    /// Builds a level. The generated walk and a handwritten impl call this rather
    /// than a struct literal, because [`Schema`] is `#[non_exhaustive]` and a
    /// struct literal is a compile error outside this crate. This mirrors the
    /// constructors [`Field`](crate::format::Field) provides.
    pub fn new(doc: Option<String>, fields: Vec<SchemaField>) -> Self {
        Self { doc, fields }
    }
}

impl SchemaField {
    /// Builds a field. The generated walk and a handwritten impl call this.
    ///
    /// `structurally_required` is whether the field's shape makes an absent
    /// field a parse error before the default is folded in, true for a required
    /// leaf, a bare string list, a required block, or a map, and false for an
    /// `Option` field or a zero-or-more block list. The [`required`](SchemaField::required)
    /// field is computed as `structurally_required && !has_default`, so a
    /// defaulted field is never required and the contradictory "required and
    /// defaulted" state cannot be built.
    pub fn new(
        name: String,
        doc: Option<String>,
        structurally_required: bool,
        has_default: bool,
        ty: SchemaType,
    ) -> Self {
        Self {
            name,
            doc,
            required: structurally_required && !has_default,
            has_default,
            default_text: None,
            ty,
            label: false,
        }
    }

    /// Marks this field as its block's label field. The derive calls it for a
    /// `#[confval(label)]` field.
    pub fn as_label(mut self) -> Self {
        self.label = true;
        self
    }

    /// Carries the field's default value rendered to text. The derive calls it
    /// for a defaulted scalar leaf, and a handwritten spec calls it the same
    /// way. See [`default_text`](SchemaField::default_text) for the per-leaf
    /// forms.
    pub fn with_default_text(mut self, text: String) -> Self {
        self.default_text = Some(text);
        self
    }
}
