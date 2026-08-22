//! Bindings and matching for a multi document configuration.
//!
//! A [`Binding`] pairs a document matcher with one root spec and one frontend.
//! [`bind`] monomorphizes the spec's schema and its validate pass into the
//! binding, so the router serves any set of specs without naming their types.
//! The [`Matcher`] decides which binding a document gets from the document's
//! file path when the client opens it.

use std::fmt;
use std::path::{Path, PathBuf};

use lsp_types::Uri;

use confval::diagnostic::Report;
use confval::format::{Fields, FromFields};
use confval::pipeline::{Validate, ValidateNested};
use confval::schema::{Schema, ToSchema};

use crate::frontend::Frontend;

/// Decides whether a binding serves a document, from the document's file path.
///
/// A matcher must not panic. On any problem it answers no match, so a routing
/// mistake surfaces as an unserved document rather than a lost one. On an
/// unwinding runtime, the notification guard drops a panic raised while the
/// server handles the open. A build with `panic = "abort"` aborts.
#[non_exhaustive]
pub enum Matcher {
    /// Every document, including one whose URI yields no file path.
    Any,
    /// The document's file name equals this name, such as `"app.hcl"`.
    FileName(String),
    /// The host decides, from the document's absolute path.
    Fn(Box<dyn Fn(&Path) -> bool + Send>),
}

impl Matcher {
    /// Whether the matcher accepts a document with this file path. `Any`
    /// needs no path. The other matchers answer no match without one.
    /// A host tests its own binding order through this, without a connection.
    pub fn matches(&self, path: Option<&Path>) -> bool {
        match self {
            Matcher::Any => true,
            Matcher::FileName(name) => path
                .and_then(Path::file_name)
                .is_some_and(|file| file == name.as_str()),
            Matcher::Fn(decide) => path.is_some_and(decide),
        }
    }
}

/// The closure variant prints a placeholder, because a function value has no
/// useful rendering.
impl fmt::Debug for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Matcher::Any => f.write_str("Any"),
            Matcher::FileName(name) => f.debug_tuple("FileName").field(name).finish(),
            Matcher::Fn(_) => f.write_str("Fn(..)"),
        }
    }
}

/// The validate pass for one root spec, erased behind a plain function
/// pointer, so the value stays `Copy` and allocation free.
#[derive(Clone, Copy)]
pub struct Validator(fn(&Fields, &mut Report));

impl Validator {
    /// The validate pass for `S`: build the spec from the fields, then run
    /// `validate_all`, appending every issue to the report.
    pub fn of<S>() -> Self
    where
        S: FromFields + Validate + ValidateNested,
    {
        Self(run::<S>)
    }

    /// Runs the pass over a parsed document.
    pub(crate) fn run(&self, fields: &Fields, report: &mut Report) {
        (self.0)(fields, report);
    }
}

impl fmt::Debug for Validator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Validator(..)")
    }
}

/// The monomorphized pass [`Validator::of`] erases.
fn run<S>(fields: &Fields, report: &mut Report)
where
    S: FromFields + Validate + ValidateNested,
{
    if let Some(spec) = S::from_fields(fields, report) {
        spec.validate_all(report);
    }
}

/// One document shape of a multi document configuration: a matcher, the
/// spec's schema, its validate pass, and the frontend that parses it.
///
/// Each binding owns its frontend value. Build one with [`bind`].
#[derive(Debug)]
pub struct Binding {
    pub(crate) matcher: Matcher,
    pub(crate) schema: Schema,
    pub(crate) validator: Validator,
    pub(crate) frontend: Box<dyn Frontend + Send>,
}

/// A binding of the root spec `S` and a frontend to the documents `matcher`
/// accepts. The schema is `S::schema()`, evaluated once here rather than per
/// document. The matcher must not panic, and on any problem it answers no
/// match. See [`Matcher`].
pub fn bind<S, F>(matcher: Matcher, frontend: F) -> Binding
where
    S: FromFields + Validate + ValidateNested + ToSchema + 'static,
    F: Frontend + Send + 'static,
{
    Binding {
        matcher,
        schema: S::schema(),
        validator: Validator::of::<S>(),
        frontend: Box::new(frontend),
    }
}

/// The file path of a document URI: a `file` scheme, compared without case,
/// with no authority or a `localhost` authority, whose non-empty path percent
/// decodes to UTF-8. The slash ahead of a Windows drive letter is stripped
/// only when a separator or the end follows the colon. Any other URI yields
/// `None`, so a path matcher answers no match for it. A remote authority
/// yields `None`, because its path would name a different file locally.
pub(crate) fn file_path(uri: &Uri) -> Option<PathBuf> {
    if !uri.scheme()?.eq_lowercase("file") {
        return None;
    }
    let host = uri.authority().map(|authority| authority.as_str());
    if host.is_some_and(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let decoded = uri.path().as_estr().decode().into_string().ok()?;
    if decoded.is_empty() {
        return None;
    }
    let path = match decoded.as_bytes() {
        [b'/', letter, b':'] | [b'/', letter, b':', b'/', ..] if letter.is_ascii_alphabetic() => {
            &decoded[1..]
        }
        _ => &decoded,
    };
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::str::FromStr;

    use confval::prelude::*;
    use confval::source::{SourceId, SourceMap};

    use crate::frontend::Insert;

    fn path_of(uri: &str) -> Option<PathBuf> {
        file_path(&Uri::from_str(uri).unwrap())
    }

    /// A frontend that parses nothing, so the binding tests need no format
    /// feature.
    #[derive(Debug)]
    struct NullFrontend;

    impl Frontend for NullFrontend {
        fn parse(&self, _: &SourceMap, _: SourceId, _: &mut Report) -> Option<Fields> {
            None
        }

        fn insert_text(&self, _: &confval::schema::SchemaField, _: &[String]) -> Insert {
            Insert::plain(String::new())
        }
    }

    /// A minimal root spec for the bind tests.
    #[derive(confval::Spec)]
    struct BindSpec {
        name: Located<String>,
    }

    impl Validate for BindSpec {
        fn validate(&self, _report: &mut Report) {}
    }

    #[test]
    fn any_matches_with_and_without_a_path() {
        // Arrange
        let matcher = Matcher::Any;

        // Act, Assert
        assert!(matcher.matches(Some(Path::new("/a/b.hcl"))));
        assert!(matcher.matches(None));
    }

    #[test]
    fn a_file_name_matcher_compares_the_last_component() {
        // Arrange
        let matcher = Matcher::FileName("snakeway.hcl".to_string());

        // Act, Assert
        assert!(matcher.matches(Some(Path::new("/etc/app/snakeway.hcl"))));
        assert!(!matcher.matches(Some(Path::new("/etc/app/other.hcl"))));
        assert!(
            !matcher.matches(None),
            "a pathless document has no file name"
        );
    }

    #[test]
    fn a_closure_matcher_decides_from_the_path_and_needs_one() {
        // Arrange
        let matcher = Matcher::Fn(Box::new(|path: &Path| {
            path.extension().is_some_and(|extension| extension == "hcl")
        }));

        // Act, Assert
        assert!(matcher.matches(Some(Path::new("/x/device.hcl"))));
        assert!(!matcher.matches(Some(Path::new("/x/device.yaml"))));
        assert!(
            !matcher.matches(None),
            "a closure is never asked without a path"
        );
    }

    #[test]
    fn a_file_uri_yields_its_decoded_path() {
        // Arrange, Act, Assert
        assert_eq!(path_of("file:///a/b.hcl"), Some(PathBuf::from("/a/b.hcl")));
        assert_eq!(
            path_of("file:///a%20b/x.hcl"),
            Some(PathBuf::from("/a b/x.hcl")),
            "a percent-encoded space decodes"
        );
    }

    #[test]
    fn a_non_file_uri_yields_no_path() {
        // Arrange, Act, Assert
        assert_eq!(path_of("untitled:Untitled-1"), None);
        assert_eq!(path_of("vscode-notebook-cell:/x.hcl"), None);
    }

    #[test]
    fn the_file_scheme_compares_without_case() {
        // Arrange, Act, Assert
        assert_eq!(path_of("FILE:///a/b.hcl"), Some(PathBuf::from("/a/b.hcl")));
        assert_eq!(path_of("File:///a/b.hcl"), Some(PathBuf::from("/a/b.hcl")));
    }

    #[test]
    fn a_remote_authority_yields_no_path() {
        // Arrange, Act, Assert
        assert_eq!(path_of("file://myhost/share/app.hcl"), None);
        assert_eq!(
            path_of("file://localhost/share/app.hcl"),
            Some(PathBuf::from("/share/app.hcl")),
            "localhost names this machine"
        );
    }

    #[test]
    fn an_empty_path_yields_no_path() {
        // Arrange, Act, Assert
        assert_eq!(path_of("file://"), None);
        assert_eq!(path_of("file:"), None);
    }

    #[test]
    fn a_path_that_does_not_decode_to_utf8_yields_no_path() {
        // Arrange, Act, Assert
        assert_eq!(path_of("file:///a%FF/x.hcl"), None);
    }

    #[test]
    fn a_windows_drive_path_drops_the_leading_slash() {
        // Arrange, Act, Assert
        assert_eq!(
            path_of("file:///C:/proj/x.hcl"),
            Some(PathBuf::from("C:/proj/x.hcl"))
        );
        assert_eq!(path_of("file:///C:"), Some(PathBuf::from("C:")));
    }

    #[test]
    fn a_posix_path_with_a_colon_component_keeps_its_leading_slash() {
        // Arrange, Act, Assert
        assert_eq!(
            path_of("file:///a:b/x.hcl"),
            Some(PathBuf::from("/a:b/x.hcl"))
        );
        assert_eq!(
            path_of("file:///c:temp/x.hcl"),
            Some(PathBuf::from("/c:temp/x.hcl"))
        );
    }

    #[test]
    fn the_matcher_debug_names_the_variant_and_hides_the_closure() {
        // Arrange
        let by_name = Matcher::FileName("app.hcl".to_string());
        let by_rule = Matcher::Fn(Box::new(|_: &Path| true));

        // Act, Assert
        assert_eq!(format!("{by_name:?}"), "FileName(\"app.hcl\")");
        assert_eq!(format!("{by_rule:?}"), "Fn(..)");
    }

    #[test]
    fn bind_captures_the_schema_and_debugs_without_the_spec_type() {
        // Arrange, Act
        let binding = bind::<BindSpec, NullFrontend>(Matcher::Any, NullFrontend);

        // Assert
        assert!(
            binding
                .schema
                .fields
                .iter()
                .any(|field| field.name == "name"),
            "the schema is evaluated at bind time"
        );
        let rendered = format!("{binding:?}");
        assert!(rendered.contains("Any"), "got: {rendered}");
        assert!(rendered.contains("Validator(..)"), "got: {rendered}");
    }

    #[test]
    fn the_validator_runs_the_spec_pipeline_into_the_report() {
        // Arrange
        let validator = Validator::of::<BindSpec>();
        let mut sources = SourceMap::new();
        let id = sources.add("<test>", "");
        let fields = Fields::new(id, confval::source::Span::new(id, 0, 0), Vec::new());
        let mut report = Report::new();

        // Act
        validator.run(&fields, &mut report);

        // Assert
        assert!(
            report
                .issues()
                .iter()
                .any(|issue| issue.message.contains("name")),
            "the missing required field reports through the erased pass, got: {:?}",
            report
                .issues()
                .iter()
                .map(|i| &i.message)
                .collect::<Vec<_>>()
        );
    }
}
