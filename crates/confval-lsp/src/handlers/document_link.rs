//! The document-link handler provides clickable file paths in the editor.
//!
//! A `Located<PathBuf>` field in a spec parses as a string and carries
//! `ScalarType::Path` in the schema. This handler walks the parsed field tree
//! alongside the schema, finds every path-typed value, resolves it to an
//! absolute URI, and returns a `DocumentLink` the editor renders as a
//! clickable underline.

use std::path::Path;
use std::str::FromStr;

use lsp_types::{DocumentLink, Uri};

use confval::format::{Field, FieldKind, Fields, Scalar, ValueKind};
use confval::schema::{ScalarType, Schema, SchemaType};

use crate::binding::file_path;
use crate::encoding::{LineIndex, PositionEncoding};

/// Collects document links for every path-typed field in the parsed tree.
pub fn document_links(
    schema: &Schema,
    fields: &Fields,
    document_uri: &Uri,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Vec<DocumentLink> {
    let base_dir = document_dir(document_uri);
    let mut links = Vec::new();
    collect(schema, fields, &base_dir, text, index, encoding, &mut links);
    links
}

fn collect(
    schema: &Schema,
    fields: &Fields,
    base_dir: &Option<std::path::PathBuf>,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    out: &mut Vec<DocumentLink>,
) {
    for schema_field in &schema.fields {
        match &schema_field.ty {
            SchemaType::Scalar {
                leaf: ScalarType::Path,
                ..
            } => {
                for field in fields.iter().filter(|f| f.name == schema_field.name) {
                    if let Some(link) = path_link(field, base_dir, text, index, encoding) {
                        out.push(link);
                    }
                }
            }
            SchemaType::Block { schema: child, .. } => {
                for field in fields.iter().filter(|f| f.name == schema_field.name) {
                    if let Some(inner) = block_fields(field) {
                        collect(child, inner, base_dir, text, index, encoding, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn path_link(
    field: &Field,
    base_dir: &Option<std::path::PathBuf>,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<DocumentLink> {
    let (span, path_str) = match &field.kind {
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Scalar(Scalar::String(s)) if !s.is_empty() => Some((value.span, s.as_str())),
            _ => None,
        },
        _ => None,
    }?;

    let resolved = resolve_path(path_str, base_dir)?;
    let target = path_to_uri(&resolved)?;
    let range = index.range_of(text, span, encoding);

    Some(DocumentLink {
        range,
        target: Some(target),
        tooltip: Some(resolved.display().to_string()),
        data: None,
    })
}

fn resolve_path(
    path_str: &str,
    base_dir: &Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let path = Path::new(path_str);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        base_dir.as_ref().map(|dir| dir.join(path))
    }
}

fn path_to_uri(path: &Path) -> Option<Uri> {
    let absolute = if path.is_absolute() {
        path.to_string_lossy().into_owned()
    } else {
        return None;
    };

    #[cfg(windows)]
    let uri_path = absolute.replace('\\', "/");
    #[cfg(not(windows))]
    let uri_path = absolute;

    let uri_string = format!("file://{uri_path}");
    Uri::from_str(&uri_string).ok()
}

fn document_dir(uri: &Uri) -> Option<std::path::PathBuf> {
    let path = file_path(uri)?;
    path.parent().map(|d| d.to_path_buf())
}

fn block_fields(field: &Field) -> Option<&Fields> {
    match &field.kind {
        FieldKind::Block(inner) => Some(inner),
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Map(inner) => Some(inner),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confval::prelude::*;
    use confval::schema::ToSchema;
    use std::path::PathBuf;

    const ENCODING: PositionEncoding = PositionEncoding::Utf8;

    #[derive(confval::Spec)]
    struct PathFixture {
        cert: Option<Located<PathBuf>>,
        hostname: Option<Located<String>>,
    }

    impl Validate for PathFixture {
        fn validate(&self, _report: &mut Report) {}
    }

    #[derive(confval::Spec)]
    struct NestedPathFixture {
        #[confval(nested)]
        tls: Option<Located<TlsFixture>>,
    }

    #[derive(confval::Spec)]
    struct TlsFixture {
        cert: Located<PathBuf>,
    }

    impl Validate for NestedPathFixture {
        fn validate(&self, _report: &mut Report) {}
    }

    impl Validate for TlsFixture {
        fn validate(&self, _report: &mut Report) {}
    }

    fn parse_hcl(text: &str) -> Option<Fields> {
        let mut sources = confval::source::SourceMap::new();
        let mut report = Report::new();
        let id = sources.add("test.hcl", text);
        confval::format::hcl::parse_hcl_fields(&sources, id, &mut report)
    }

    fn parse_json(text: &str) -> Option<Fields> {
        let mut sources = confval::source::SourceMap::new();
        let mut report = Report::new();
        let id = sources.add("test.json", text);
        confval::format::json::parse_json_fields(&sources, id, &mut report)
    }

    fn doc_uri(path: &str) -> Uri {
        Uri::from_str(&format!("file://{path}")).unwrap()
    }

    #[test]
    fn an_absolute_path_produces_a_link() {
        // Arrange
        let text = "cert = \"/etc/tls/cert.pem\"\n";
        let fields = parse_hcl(text).unwrap();
        let schema = PathFixture::schema();
        let uri = doc_uri("/home/user/server.hcl");
        let index = LineIndex::new(text);

        // Act
        let links = document_links(&schema, &fields, &uri, text, &index, ENCODING);

        // Assert
        assert_eq!(links.len(), 1);
        let target = links[0].target.as_ref().unwrap().as_str();
        assert!(target.ends_with("/etc/tls/cert.pem"), "got: {target}");
        assert_eq!(links[0].tooltip.as_deref(), Some("/etc/tls/cert.pem"));
    }

    #[test]
    fn a_relative_path_resolves_against_the_document_directory() {
        // Arrange
        let text = "cert = \"certs/server.pem\"\n";
        let fields = parse_hcl(text).unwrap();
        let schema = PathFixture::schema();
        let uri = doc_uri("/home/user/config/server.hcl");
        let index = LineIndex::new(text);

        // Act
        let links = document_links(&schema, &fields, &uri, text, &index, ENCODING);

        // Assert
        assert_eq!(links.len(), 1);
        let target = links[0].target.as_ref().unwrap().as_str();
        assert!(
            target.ends_with("/home/user/config/certs/server.pem"),
            "got: {target}"
        );
    }

    #[test]
    fn an_empty_string_produces_no_link() {
        // Arrange
        let text = "cert = \"\"\n";
        let fields = parse_hcl(text).unwrap();
        let schema = PathFixture::schema();
        let uri = doc_uri("/home/user/server.hcl");
        let index = LineIndex::new(text);

        // Act
        let links = document_links(&schema, &fields, &uri, text, &index, ENCODING);

        // Assert
        assert!(links.is_empty());
    }

    #[test]
    fn a_path_inside_a_map_valued_block_produces_a_link() {
        // Arrange
        // JSON parses a nested block as a map value, so the link appears only
        // when the handler enters a map-valued block.
        let text = "{ \"tls\": { \"cert\": \"/etc/tls/cert.pem\" } }";
        let fields = parse_json(text).unwrap();
        let schema = NestedPathFixture::schema();
        let uri = doc_uri("/home/user/server.json");
        let index = LineIndex::new(text);

        // Act
        let links = document_links(&schema, &fields, &uri, text, &index, ENCODING);

        // Assert
        assert_eq!(links.len(), 1, "the cert path inside the map block links");
        let target = links[0].target.as_ref().unwrap().as_str();
        assert!(target.ends_with("/etc/tls/cert.pem"), "got: {target}");
    }

    #[test]
    fn a_non_path_field_produces_no_link() {
        // Arrange
        let text = "hostname = \"127.0.0.1\"\n";
        let fields = parse_hcl(text).unwrap();
        let schema = PathFixture::schema();
        let uri = doc_uri("/home/user/server.hcl");
        let index = LineIndex::new(text);

        // Act
        let links = document_links(&schema, &fields, &uri, text, &index, ENCODING);

        // Assert
        assert!(links.is_empty());
    }

    #[test]
    fn a_path_inside_a_nested_block_produces_a_link() {
        // Arrange
        let text = "tls {\n  cert = \"/etc/cert.pem\"\n}\n";
        let fields = parse_hcl(text).unwrap();
        let schema = NestedPathFixture::schema();
        let uri = doc_uri("/home/user/server.hcl");
        let index = LineIndex::new(text);

        // Act
        let links = document_links(&schema, &fields, &uri, text, &index, ENCODING);

        // Assert
        assert_eq!(links.len(), 1);
        let target = links[0].target.as_ref().unwrap().as_str();
        assert!(target.ends_with("/etc/cert.pem"), "got: {target}");
    }

    #[test]
    fn no_fields_produces_no_links() {
        // Arrange
        let text = "\n";
        let fields = parse_hcl(text).unwrap();
        let schema = PathFixture::schema();
        let uri = doc_uri("/home/user/server.hcl");
        let index = LineIndex::new(text);

        // Act
        let links = document_links(&schema, &fields, &uri, text, &index, ENCODING);

        // Assert
        assert!(links.is_empty());
    }
}
