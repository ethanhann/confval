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
use confval::schema::SchemaField;
use confval::source::{SourceId, SourceMap};

use crate::resolve::{resolve_in_tree, value_span_token};
use crate::scan::{resolve_in_text, resolve_in_yaml};

/// The raw-text recovery a frontend's syntax needs.
///
/// The clean-buffer walk reads the parsed [`Fields`]. This selects the
/// reconstruction when there is no tree, and for [`Indentation`](Recovery::Indentation)
/// in both parse states, because block YAML's parsed spans do not cover a pending
/// body position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The character a frontend writes between a name and its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSeparator {
    /// `name = value`: HCL, TOML.
    Equals,
    /// `name: value`: JSON, YAML.
    Colon,
    /// `name value`: KDL.
    Whitespace,
}

/// The kind of position a cursor sits in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionKind {
    /// A body position, where an attribute name or a block type is legal.
    Body,
    /// An attribute-value position for the named field.
    AttributeValue {
        /// The name of the field whose value the cursor sits in.
        field: String,
    },
    /// A block-label position for the enclosing block. Resolution does not yet
    /// produce this variant. It is reserved for label completion.
    BlockLabel,
}

/// The resolved query result the handlers read.
///
/// It names the schema path from the root to the block that encloses the cursor,
/// the kind of position the cursor sits in, and the byte range of the identifier
/// or value under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorContext {
    /// The schema path from the root to the block that encloses the cursor, each
    /// element the field name of a block the cursor sits inside.
    pub path: Vec<String>,
    /// The kind of position the cursor sits in.
    pub kind: PositionKind,
    /// The byte range in the current text that a completion replaces: the
    /// identifier or value under the cursor, or a zero-width range at the cursor
    /// when it sits on no token. It is scanned from the current text, so it stays
    /// valid and on the cursor's line even when the buffer does not parse.
    pub token: (usize, usize),
}

impl CursorContext {
    /// A body position at `path` with the given replace token.
    pub(crate) fn body(path: Vec<String>, token: (usize, usize)) -> Self {
        Self {
            path,
            kind: PositionKind::Body,
            token,
        }
    }

    /// An attribute-value position for `field` at `path`.
    pub(crate) fn attribute_value(path: Vec<String>, field: String, token: (usize, usize)) -> Self {
        Self {
            path,
            kind: PositionKind::AttributeValue { field },
            token,
        }
    }
}

/// The one format-dependent trait.
///
/// A frontend binds one format's parse function and insert text. Parsing and
/// resolution reuse `confval`'s machinery, so the block-structured formats share
/// the default [`parse_tree`](Frontend::parse_tree) and [`resolve`](Frontend::resolve).
pub trait Frontend {
    /// Parses the buffer into the neutral field model, appending to `report`.
    /// Delegates to the format's existing `confval` parse function, so
    /// diagnostics reuse the real pipeline rather than an approximation.
    fn parse(&self, sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields>;

    /// Parses `text` into the neutral [`Fields`]. The document store holds the
    /// current parse, or `None` when the text does not parse. A throwaway
    /// [`SourceMap`] holds the text, because resolution reads only byte offsets,
    /// which are the same in any map.
    fn parse_tree(&self, text: &str) -> Option<Fields> {
        let mut sources = SourceMap::new();
        let id = sources.add("<buffer>", text);
        let mut report = Report::new();
        self.parse(&sources, id, &mut report)
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
        if matches!(self.recovery(), Recovery::Indentation) {
            let mut context = resolve_in_yaml(text, offset);
            // The reader reads the path and kind from indentation, but a parsed
            // value's exact span replaces the whole value, so completing a
            // spaced or quoted value does not stop at a space.
            if let Some(tree) = tree {
                let field = match &context.kind {
                    PositionKind::AttributeValue { field } => Some(field.clone()),
                    _ => None,
                };
                if let Some(field) = field
                    && let Some(token) = value_span_token(tree, &context.path, &field, text)
                {
                    context.token = token;
                }
            }
            return context;
        }
        match tree {
            Some(tree) => resolve_in_tree(tree, text, offset, self.block_span_covers_body()),
            None => resolve_in_text(
                text,
                offset,
                self.recovery(),
                self.value_separator(),
                self.hash_is_comment(),
            ),
        }
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

    /// Whether `#` starts a line comment (HCL) rather than a value token (KDL
    /// writes booleans `#true`). The text-based recovery reads this when it scans
    /// blocks. The default is `true`.
    fn hash_is_comment(&self) -> bool {
        true
    }

    /// Renders a field's insert text in the format, reading the field's
    /// `SchemaType` to write a scalar as the format's `name = value` form or a
    /// block as its block form. `path` is the enclosing block path, which a
    /// header-based format (TOML) uses to qualify a nested block header.
    ///
    /// A brace-delimited block insert places a `$0` where the cursor belongs,
    /// inside the body. The completion handler emits it as a snippet tab stop
    /// when the client supports snippets, or removes it otherwise, so the marker
    /// never reaches a buffer literally.
    fn insert_text(&self, field: &SchemaField, path: &[String]) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hcl, Yaml};

    #[test]
    fn a_brace_frontend_uses_the_default_trait_settings() {
        // Arrange
        let frontend = Hcl;

        // Act
        let recovery = frontend.recovery();

        // Assert
        assert_eq!(recovery, Recovery::Braces);
        assert_eq!(frontend.value_separator(), ValueSeparator::Equals);
        assert!(frontend.hash_is_comment());
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
