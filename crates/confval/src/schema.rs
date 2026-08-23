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
    /// field's leaf type to know what it holds. The evaluation runs wherever
    /// `schema()` runs, including inside a long-running language server, so a
    /// default expression must not panic and must not carry side effects.
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
    #[non_exhaustive]
    Scalar {
        /// The leaf's config-level type.
        leaf: ScalarType,
        /// The mechanical constraint the field records, or `None`.
        constraint: Option<Constraint>,
    },
    /// A list of strings, with the constraint each element declares, if any.
    ///
    /// The constraint describes one element rather than the list, so a closed
    /// set means every entry must be one of those words. Only
    /// [`Keywords`](Constraint::Keywords) is meaningful here, because the derive
    /// records nothing else on a list. A constraint that bounds the list itself,
    /// such as a length, would need its own slot rather than this one.
    #[non_exhaustive]
    StringList {
        /// The mechanical constraint each element records, or `None`.
        constraint: Option<Constraint>,
    },
    /// A nested block. `repeated` is true for a zero-or-more block list.
    #[non_exhaustive]
    Block {
        /// The child level's schema.
        schema: Box<Schema>,
        /// Whether the block is a zero-or-more list rather than a single block.
        repeated: bool,
    },
    /// An open-ended, string-keyed map with string values. Keys are open, so the
    /// node names no key or value type.
    StringMap,
}

impl SchemaType {
    /// A scalar leaf of `leaf`, recording `constraint`.
    pub fn scalar(leaf: ScalarType, constraint: Option<Constraint>) -> Self {
        Self::Scalar { leaf, constraint }
    }

    /// A string list whose elements record `constraint`.
    pub fn string_list(constraint: Option<Constraint>) -> Self {
        Self::StringList { constraint }
    }

    /// A nested block holding `schema`, repeated when `repeated` is true.
    pub fn block(schema: Schema, repeated: bool) -> Self {
        Self::Block {
            schema: Box::new(schema),
            repeated,
        }
    }

    /// An open-ended, string-keyed map with string values.
    pub fn string_map() -> Self {
        Self::StringMap
    }

    /// The constraint this type records, or `None`.
    ///
    /// A scalar records one for its value and a string list records one for
    /// each element, so a reader that renders a constraint takes it from here
    /// rather than naming both variants.
    pub fn constraint(&self) -> Option<&Constraint> {
        match self {
            Self::Scalar { constraint, .. } | Self::StringList { constraint } => {
                constraint.as_ref()
            }
            _ => None,
        }
    }
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
    /// The allowed strings of a `keyword_enum!`, in declaration order. The
    /// variant stays open, because a `#[non_exhaustive]` tuple variant cannot
    /// be matched outside this crate, and a reader has to bind the set. A
    /// richer keyword constraint arrives as a new variant instead.
    Keywords(&'static [&'static str]),
    /// An inclusive numeric range, with its bounds rendered to text for a
    /// text-facing hover or diagnostic line. `help` carries the constraint's
    /// custom help line for the hover, or `None`.
    #[non_exhaustive]
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
    #[non_exhaustive]
    References {
        /// The config field name of the referenced labeled block.
        block: &'static str,
    },
}

impl Constraint {
    /// Builds a keyword-set constraint. The generated walk and a handwritten
    /// impl call these constructors rather than struct literals, because the
    /// variants are `#[non_exhaustive]` so a later field lands without a
    /// break, mirroring the [`SchemaType`] constructors.
    pub fn keywords(words: &'static [&'static str]) -> Self {
        Self::Keywords(words)
    }

    /// Builds an inclusive numeric range with its rendered bounds.
    pub fn range(
        min: String,
        max: String,
        units: Option<&'static str>,
        help: Option<&'static str>,
    ) -> Self {
        Self::Range {
            min,
            max,
            units,
            help,
        }
    }

    /// Builds a reference to the labels of the named block field.
    pub fn references(block: &'static str) -> Self {
        Self::References { block }
    }
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
    /// Builds an optional field with no default. The generated walk and a
    /// handwritten impl call this, then mark the field through the builders:
    /// [`required`](SchemaField::required) when the field's shape makes an
    /// absent field a parse error, and
    /// [`with_default`](SchemaField::with_default) when a default fills it.
    /// A defaulted field stays unrequired whichever order the two are applied
    /// in, so the contradictory "required and defaulted" state cannot be
    /// built.
    pub fn new(name: String, doc: Option<String>, ty: SchemaType) -> Self {
        Self {
            name,
            doc,
            required: false,
            has_default: false,
            default_text: None,
            ty,
            label: false,
        }
    }

    /// Marks the field structurally required: its shape makes an absent field
    /// a parse error before a default is folded in, true for a required leaf,
    /// a bare string list, a required block, or a map. A field that also
    /// declares a default stays unrequired.
    pub fn required(mut self) -> Self {
        self.required = !self.has_default;
        self
    }

    /// Marks the field defaulted. A defaulted field can be absent, so this
    /// also clears the required flag.
    pub fn with_default(mut self) -> Self {
        self.has_default = true;
        self.required = false;
        self
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
