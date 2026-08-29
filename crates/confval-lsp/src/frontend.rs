//! The [`Frontend`] trait and the [`CursorContext`] its resolution produces.
//!
//! A frontend delegates parsing to `confval`, resolves a byte offset to a cursor
//! context, and renders a field's insert text in its format. Parsing and insert
//! rendering reuse `confval`'s machinery, and HCL, TOML, KDL, and JSON resolve
//! through one shared walk, so every frontend uses the default
//! [`resolve`](Frontend::resolve), which routes an indentation format (YAML) to
//! the YAML reader.

use confval::diagnostic::Report;
use confval::format::Fields;
use confval::schema::{ScalarType, SchemaField};
use confval::source::{SourceId, SourceMap};

use crate::resolve::{bodies_along_path, resolve_in_tree, value_span_in};
use crate::scan::{
    TextRecovery, innermost_is_array, resolve_in_text, resolve_in_yaml, starts_new_sequence_element,
};

/// The raw-text recovery a frontend's syntax needs.
///
/// The clean-buffer walk reads the parsed [`Fields`]. This selects the
/// reconstruction when there is no tree, and for [`Indentation`](Recovery::Indentation)
/// in both parse states, because block YAML's parsed spans do not cover a pending
/// body position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Recovery {
    /// A brace-delimited block language: HCL, KDL.
    Braces,
    /// A header-addressed table language: TOML.
    Header,
    /// A brace-delimited object language with quoted keys: JSON.
    Object,
    /// An indentation-nested language: YAML.
    Indentation,
}

impl Recovery {
    /// Whether a block in this format ends at a closing brace, so its parsed
    /// span covers the whole block. A header or indentation format's block
    /// span runs to the next sibling instead.
    pub fn closes_with_brace(self) -> bool {
        matches!(self, Recovery::Braces | Recovery::Object)
    }

    /// The raw-text scan's own dispatch, or `None` for an indentation format,
    /// whose reader covers both parse states before the text scan is reached.
    /// The scan's enum has no indentation variant, so it cannot be asked to
    /// recover a format its readers do not cover.
    pub(crate) fn text(self) -> Option<TextRecovery> {
        match self {
            Recovery::Braces => Some(TextRecovery::Braces),
            Recovery::Header => Some(TextRecovery::Header),
            Recovery::Object => Some(TextRecovery::Object),
            Recovery::Indentation => None,
        }
    }
}

/// The character a frontend writes between a name and its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueSeparator {
    /// `name = value`: HCL, TOML.
    Equals,
    /// `name: value`: JSON, YAML.
    Colon,
    /// `name value`: KDL.
    Whitespace,
}

/// A field's rendered insert, with the format's edit geometry.
///
/// The completion handler applies `absorb` to the replace range, so the format
/// decides what a typed prefix character means rather than the handler
/// inferring it from the insert string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Insert {
    /// The text the completion inserts. A block insert places a `$0` where the
    /// cursor belongs.
    pub text: String,
    /// What the edit absorbs to the left of the replace range.
    pub absorb: Absorb,
    /// Whether `text` contains snippet markers. The producer declares it, so
    /// the handler never infers snippet-ness from the string.
    pub snippet: bool,
}

impl Insert {
    /// A literal insert with no left absorption.
    pub fn plain(text: String) -> Self {
        Self::marked(text, false)
    }

    /// A snippet insert with no left absorption.
    pub fn snippet(text: String) -> Self {
        Self::marked(text, true)
    }

    fn marked(text: String, snippet: bool) -> Self {
        Self {
            text,
            absorb: Absorb::None,
            snippet,
        }
    }
}

/// What a completion edit absorbs to the left of its replace range.
///
/// A format whose insert re-renders a delimiter the operator has already typed
/// absorbs that delimiter, so accepting the item does not double it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Absorb {
    /// Nothing is absorbed.
    None,
    /// A run of this byte directly before the range. For example, a TOML
    /// header re-renders the `[` run, so `[lim` completes to `[limits]` rather
    /// than `[[limits]`.
    Run(u8),
    /// One occurrence of this byte directly before the range. For example, a
    /// JSON member re-renders its opening `"`, so `"por` completes to
    /// `"port": ` rather than a doubled quote.
    One(u8),
}

/// The kind of position a cursor is in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PositionKind {
    /// A body position, where an attribute name or a block type is legal.
    Body,
    /// An attribute-value position for the named field.
    AttributeValue {
        /// The name of the field whose value the cursor is in.
        field: String,
    },
    /// A block-label position, the region between a block's type and its body in
    /// HCL and KDL. Completion offers nothing here, because an author names a
    /// block freely, and hover names the block. The label's byte span is the
    /// context's `token`.
    BlockLabel {
        /// The block's type, its field name.
        block: String,
    },
}

/// The resolved query result the handlers read.
///
/// It names the schema path from the root to the block that encloses the cursor,
/// the kind of position the cursor is in, and the byte range of the identifier
/// or value under the cursor.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CursorContext {
    /// The schema path from the root to the block that encloses the cursor, each
    /// element the field name of a block the cursor is inside.
    pub path: Vec<String>,
    /// The kind of position the cursor is in.
    pub kind: PositionKind,
    /// The byte range in the current text that a completion replaces: the
    /// identifier or value under the cursor, or a zero-width range at the cursor
    /// when it is on no token. It is scanned from the current text, so it stays
    /// valid and on the cursor's line even when the buffer does not parse.
    pub token: (usize, usize),
    /// The text of `token`, kept so the handlers read the typed prefix
    /// without holding the buffer.
    pub token_text: String,
    /// Whether a body completion here opens a new element of a repeated block
    /// rather than adding a field to the element the cursor is in. The
    /// answer is syntactic, and the frontend resolves it. The handlers consult
    /// it only behind the schema's repeated-block check, so the default at an
    /// unrepeated position is never read.
    pub new_element: bool,
    /// The fields of the block instance the cursor is in, when the buffer
    /// parsed. The handlers read the already-set state and the hover state from
    /// it, so a repeated block addresses the instance the cursor is in rather
    /// than the first. A pending body, a key whose body the tree does not hold
    /// yet, has an empty body, because nothing is set there. It is `None`
    /// only on the text recovery path, which has no parsed instance.
    pub resolved_body: Option<Fields>,
    /// The bodies of the enclosing block instances along `path`, root first,
    /// one per path segment, recorded by the same descent that fills
    /// `resolved_body`. The reference handlers search them outward for the
    /// scope that declares a reference target, the rule the pipeline's
    /// reference pass applies. Empty on the text recovery path.
    pub ancestors: Vec<Fields>,
}

/// Equality ignores the resolved body, the ancestors, the token text, and the
/// new-element flag, because they are resolution outputs rather than position
/// identity.
/// The path, the kind, and the token identify the position.
impl PartialEq for CursorContext {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.kind == other.kind && self.token == other.token
    }
}

impl Eq for CursorContext {}

impl CursorContext {
    /// A context at `path` of the given kind and replace token, with no resolved
    /// body. Resolution fills the body in afterward when the buffer parsed.
    fn at(path: Vec<String>, kind: PositionKind, token: (usize, usize)) -> Self {
        Self {
            path,
            kind,
            token,
            token_text: String::new(),
            new_element: false,
            resolved_body: None,
            ancestors: Vec::new(),
        }
    }

    /// A body position at `path` with the given replace token.
    pub(crate) fn body(path: Vec<String>, token: (usize, usize)) -> Self {
        Self::at(path, PositionKind::Body, token)
    }

    /// An attribute-value position for `field` at `path`.
    pub(crate) fn attribute_value(path: Vec<String>, field: String, token: (usize, usize)) -> Self {
        Self::at(path, PositionKind::AttributeValue { field }, token)
    }

    /// A block-label position for the `block` type at `path`. The token is the
    /// label's byte span.
    pub(crate) fn block_label(path: Vec<String>, block: String, token: (usize, usize)) -> Self {
        Self::at(path, PositionKind::BlockLabel { block }, token)
    }
}

/// The one format-dependent trait.
///
/// A frontend binds one format's parse function and insert text. Parsing and
/// resolution reuse `confval`'s machinery, so the block-structured formats share
/// the default [`parse_tree`](Frontend::parse_tree) and [`resolve`](Frontend::resolve).
/// `Debug` is a supertrait so a binding that stores a frontend behind `dyn`
/// can render itself in errors and logs.
pub trait Frontend: std::fmt::Debug {
    /// Parses the buffer into the neutral field model, appending to `report`.
    /// Delegates to the format's existing `confval` parse function, so
    /// diagnostics reuse the real pipeline rather than an approximation.
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields>;

    /// Parses `text` into the neutral [`Fields`] and the report the parse
    /// produced. The document store holds both, so diagnostics reuse the parse
    /// instead of running it again. A throwaway [`SourceMap`] holds the text,
    /// because resolution reads only byte offsets, which are the same in any
    /// map.
    fn parse_buffer(&self, text: &str) -> (Option<Fields>, Report) {
        let mut sources = SourceMap::new();
        let id = sources.add("<buffer>", text);
        let mut report = Report::new();
        let fields = self.parse(&sources, id, &mut report);
        (fields, report)
    }

    /// Parses `text` into the neutral [`Fields`], or `None` when the text does
    /// not parse, dropping the report.
    fn parse_tree(&self, text: &str) -> Option<Fields> {
        self.parse_buffer(text).0
    }

    /// Resolves a byte offset to the cursor context.
    ///
    /// When `tree` is present, the buffer parsed and resolution walks it, so the
    /// spans align with the text exactly. When it is absent, the buffer did not
    /// parse, so resolution reconstructs the block path and the position kind
    /// from the raw text, whose offsets are always current.
    fn resolve(&self, tree: Option<&Fields>, text: &str, offset: usize) -> CursorContext {
        // An indentation language reads its structure from the raw text in both
        // parse states, because a block mapping's parsed span stops at its last
        // child and an empty key parses as null, so the tree does not cover a
        // pending body position.
        let mut context = if let Some(recovery) = self.recovery().text() {
            match tree {
                Some(tree) => resolve_in_tree(tree, text, offset, self.block_span_covers_body()),
                None => resolve_in_text(
                    text,
                    offset,
                    recovery,
                    self.value_separator(),
                    self.line_comments(),
                ),
            }
        } else {
            let mut context = resolve_in_yaml(text, offset);
            // YAML resolves its path from indentation, so the instance body is
            // read from the tree here, the second site the tree walk does not
            // cover. The body follows the path, choosing a sequence element by
            // offset, so a repeated block addresses the correct element and a
            // pending body reads as empty. A parsed value's exact span from that
            // body replaces the whole value, so completing a spaced or quoted
            // value does not stop at a space, inside an element as well.
            if let Some(tree) = tree {
                let mut bodies =
                    bodies_along_path(tree, &context.path, offset, self.block_span_covers_body());
                let body = bodies.pop().unwrap_or_else(|| tree.clone());
                if let PositionKind::AttributeValue { field } = &context.kind
                    && let Some(token) = value_span_in(&body, field, text)
                {
                    context.token = token;
                }
                context.resolved_body = Some(body);
                context.ancestors = bodies;
            }
            context
        };
        // Every context field is resolved here, once, so the handlers stop
        // scanning the buffer. The syntactic new-element predicates live with
        // their formats' scanners. A YAML element begins on a fresh line
        // aligned with the sequence dash, and a JSON element begins directly
        // in an array.
        context.new_element = match self.recovery() {
            Recovery::Indentation => starts_new_sequence_element(text, context.token),
            Recovery::Object => innermost_is_array(text, context.token.0),
            _ => false,
        };
        context.token_text = text
            .get(context.token.0..context.token.1)
            .unwrap_or_default()
            .to_string();
        context
    }

    /// Whether a block's span covers its whole body.
    ///
    /// A brace-delimited block (HCL, KDL) spans its body, so its end bounds the
    /// body. A header-only block (a TOML table) spans only its header, so
    /// resolution extends its body to the next sibling or the end of the
    /// enclosing level. The default is `true`.
    fn block_span_covers_body(&self) -> bool {
        true
    }

    /// The raw-text recovery the frontend's syntax needs. The text recovery
    /// dispatches on this to reconstruct the enclosing path. The default is
    /// [`Recovery::Braces`].
    fn recovery(&self) -> Recovery {
        Recovery::Braces
    }

    /// The character the frontend writes between a name and its value. The text
    /// recovery reads this to detect a value position when the buffer does not
    /// parse. The default is [`ValueSeparator::Equals`].
    fn value_separator(&self) -> ValueSeparator {
        ValueSeparator::Equals
    }

    /// The line-comment starts the format reads outside a string. The
    /// text-based recovery skips a comment when it scans blocks and refuses a
    /// value position inside one. The default is HCL's pair. KDL drops the
    /// hash, because it writes booleans `#true`.
    fn line_comments(&self) -> &'static [&'static str] {
        &["#", "//"]
    }

    /// Renders a field's insert in the format, reading the field's
    /// `SchemaType` to write a scalar as the format's `name = value` form or a
    /// block as its block form. `path` is the enclosing block path, which a
    /// header-based format (TOML) uses to qualify a nested block header.
    ///
    /// The returned [`Insert`] holds the edit geometry beside the text: what
    /// the edit absorbs to its left. A brace-delimited block insert places a
    /// `$0` where the cursor belongs, inside the body. The completion handler
    /// emits it as a snippet tab stop when the client supports snippets, or
    /// removes it otherwise, so the marker never reaches a buffer literally.
    fn insert_text(&self, field: &SchemaField, path: &[String]) -> Insert;

    /// Wraps a field insert as a new element of a repeated block. A YAML
    /// sequence element takes a `- ` marker, and a JSON array element is an
    /// object. A wrap that adds a snippet marker also sets the insert's
    /// `snippet` flag, so the grammar stays declared by its producer. The
    /// default leaves the insert unchanged, because a brace or header format
    /// never opens an element from a body position.
    fn wrap_element(&self, insert: Insert) -> Insert {
        insert
    }

    /// Renders a default value as the format's literal text, from the leaf and
    /// the kept text. The pre-filled insert, the preselected value item,
    /// and the code-action edit all go through it, so the editor writes the
    /// value the way the format reads it. The default quotes a string and a
    /// path and passes the rest through. KDL overrides the boolean forms.
    fn default_literal(&self, leaf: &ScalarType, text: &str) -> String {
        match leaf {
            ScalarType::String | ScalarType::Path => quoted_literal(text),
            _ => text.to_string(),
        }
    }
}

/// A quoted string literal with its inner quotes, backslashes, and control
/// characters escaped, the forms every frontend's double-quoted string reads.
pub(crate) fn quoted_literal(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 2);
    escaped.push('"');
    for character in text.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hcl, Yaml};

    #[test]
    fn cursor_context_equality_compares_path_kind_and_token() {
        // Arrange
        let base = CursorContext::body(vec!["a".to_string()], (0, 1));
        let same = CursorContext::body(vec!["a".to_string()], (0, 1));
        let other_path = CursorContext::body(vec!["b".to_string()], (0, 1));
        let other_kind =
            CursorContext::attribute_value(vec!["a".to_string()], "f".to_string(), (0, 1));
        let other_token = CursorContext::body(vec!["a".to_string()], (0, 2));

        // Act, Assert
        assert_eq!(base, same);
        assert_ne!(base, other_path);
        assert_ne!(base, other_kind);
        assert_ne!(base, other_token);
    }

    #[test]
    fn a_brace_frontend_uses_the_default_trait_settings() {
        // Arrange
        let frontend = Hcl;

        // Act
        let recovery = frontend.recovery();

        // Assert
        assert_eq!(recovery, Recovery::Braces);
        assert_eq!(frontend.value_separator(), ValueSeparator::Equals);
        assert_eq!(frontend.line_comments(), &["#", "//"]);
        assert!(frontend.block_span_covers_body());
    }

    #[test]
    fn parse_tree_returns_the_fields_for_a_valid_buffer() {
        // Arrange
        let frontend = Hcl;

        // Act
        let tree = frontend.parse_tree("port = 8080\n");

        // Assert
        assert!(tree.is_some(), "a valid buffer parses into fields");
    }

    #[test]
    fn parse_tree_returns_none_for_an_invalid_buffer() {
        // Arrange
        let frontend = Hcl;

        // Act
        let tree = frontend.parse_tree("port = = 8080\n");

        // Assert
        assert!(tree.is_none(), "an invalid buffer does not parse");
    }

    #[test]
    fn a_brace_frontend_with_a_tree_resolves_a_value_through_the_tree_walk() {
        // Arrange
        let frontend = Hcl;
        let text = "port = 8080\n";
        let offset = text.find("8080").expect("value present") + 1;

        // Act
        let context = frontend.resolve(frontend.parse_tree(text).as_ref(), text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(
            context.kind,
            PositionKind::AttributeValue {
                field: "port".to_string()
            }
        );
    }

    #[test]
    fn a_brace_frontend_without_a_tree_resolves_through_the_text_scan() {
        // Arrange
        let frontend = Hcl;
        let text = "hostname = \"api\"\nwork";

        // Act
        let context = frontend.resolve(None, text, text.len());

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
        let (start, end) = context.token;
        assert_eq!(&text[start..end], "work");
    }

    #[test]
    fn an_indentation_frontend_with_a_tree_replaces_the_whole_parsed_value() {
        // Arrange
        // A parsed YAML value's exact span replaces the whole value, so a quoted
        // value with a space is not split at the space.
        let frontend = Yaml;
        let text = "limits:\n  mode: \"log loud\"\n";
        let offset = text.find("log").expect("value present");

        // Act
        let context = frontend.resolve(frontend.parse_tree(text).as_ref(), text, offset);

        // Assert
        assert_eq!(
            context.kind,
            PositionKind::AttributeValue {
                field: "mode".to_string()
            }
        );
        let (start, end) = context.token;
        assert_eq!(&text[start..end], "\"log loud\"");
    }

    #[test]
    fn an_indentation_frontend_with_a_tree_keeps_the_body_token_at_a_key_position() {
        // Arrange
        // A body position is not an attribute value, so the tree's value span is
        // never consulted and the indentation reader's identifier token stands.
        let frontend = Yaml;
        let text = "limits:\n  mode: enforce\n";
        let offset = text.find("mode").expect("key present") + 1;

        // Act
        let context = frontend.resolve(frontend.parse_tree(text).as_ref(), text, offset);

        // Assert
        assert_eq!(context.path, vec!["limits".to_string()]);
        assert_eq!(context.kind, PositionKind::Body);
        let (start, end) = context.token;
        assert_eq!(&text[start..end], "mode");
    }

    #[test]
    fn the_default_literal_quotes_strings_and_paths_and_passes_numbers() {
        // Arrange
        let frontend = Hcl;

        // Act, Assert
        assert_eq!(frontend.default_literal(&ScalarType::Int, "4"), "4");
        assert_eq!(frontend.default_literal(&ScalarType::Bool, "true"), "true");
        assert_eq!(
            frontend.default_literal(&ScalarType::Path, "/etc/app.conf"),
            "\"/etc/app.conf\""
        );
        assert_eq!(
            frontend.default_literal(&ScalarType::String, "a \"b\" \\c"),
            "\"a \\\"b\\\" \\\\c\""
        );
    }

    #[test]
    fn an_indentation_frontend_without_a_tree_reads_the_raw_text() {
        // Arrange
        // A two-document YAML stream cannot hold one configuration, so it does not
        // parse and resolution reads the path and kind from indentation alone.
        let frontend = Yaml;
        let text = "hostname: api\n---\nfoo: bar\n";
        assert!(
            frontend.parse_tree(text).is_none(),
            "a two-document stream does not parse"
        );
        let offset = text.find("foo").expect("key present") + 1;

        // Act
        let context = frontend.resolve(None, text, offset);

        // Assert
        assert_eq!(context.path, Vec::<String>::new());
        assert_eq!(context.kind, PositionKind::Body);
    }
}
