//! KDL frontend: parses KDL 2.0 text into the format-neutral [`Fields`] tree.
//!
//! This module's whole job is the conversion from kdl-rs's document tree to the
//! owned, format-neutral model in [`field`](crate::format::field). Once
//! [`parse_kdl`] hands back a `Fields`, every span has been captured and no
//! kdl-rs type escapes. The leaf parsers, the derive-generated walks, and the
//! handwritten [`FromFields`] impls all work against the neutral model.
//!
//! The write path, [`emit_kdl`], lives in the sibling `emit` module.
//!
//! A KDL node maps to one field by its shape. A node with only arguments is a
//! value: one scalar argument is a scalar, more are a sequence, and a bare node
//! is an empty sequence, the only spelling KDL has for an empty list. A node
//! with properties or children is a block, with the properties as leading
//! fields, so `tls cert="a.pem"` and `tls { cert "a.pem" }` reach the same
//! `FromFields` impl. Repeated same-named nodes stay separate fields, and the
//! spec-side walk resolves them: a list field accumulates them and a
//! single-value field reports a duplicate.
//!
//! Behavior contract:
//!
//! - Parsing uses the KDL 2.0 grammar alone, with no 1.0 fallback, so every
//!   diagnostic comes from one grammar.
//! - Syntax errors are pushed to the report one per kdl-rs diagnostic, each at
//!   its own span, and parsing returns `None`.
//! - Values outside the neutral model (`#null`, an integer beyond `i64`)
//!   become [`ValueKind::Other`] carrying a diagnostic label, so they surface
//!   as ordinary type mismatches at the field that used them.
//! - Arguments on a node that also has properties or children have no home in
//!   the model, which carries no block labels. They are reported and the block
//!   is still produced, so the walk continues.
//! - Type annotations such as `(u8)123` carry no information the model can
//!   hold and are read through.

use crate::diagnostic::Report;
use crate::format::field::{Field, FieldKind, Fields, FromFields, Scalar, Value, ValueKind};
use crate::source::{SourceId, SourceMap, Span};
use kdl::{KdlDocument, KdlEntry, KdlIdentifier, KdlNode, KdlValue};

mod emit;
mod text;
pub use emit::emit_kdl;

/// Parses one registered source into the neutral [`Fields`] tree.
///
/// When you assemble configuration from several sources, you hold the returned
/// `Fields`, merge it with the others, and run [`FromFields`] once on the
/// merged result. A syntax error, the only failure that yields no tree, is
/// reported and returns `None`, one issue per kdl-rs diagnostic. Field-level
/// problems are reported but do not stop the parse, so a tree that parsed
/// still reaches validation.
pub fn parse_kdl_fields(sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
    let Some(source) = sources.get(id) else {
        report
            .error("internal error: parse_kdl_fields called with an unregistered source id")
            .emit();
        return None;
    };
    match KdlDocument::parse_v2(&source.text) {
        Ok(document) => {
            let enclosing = Span::new(id, 0, source.text.len() as u32);
            Some(fields_of_document(&document, enclosing, id, report))
        }
        Err(error) => {
            for diagnostic in &error.diagnostics {
                // A diagnostic with no message still names the failure class,
                // mirroring kdl-rs's own fallback without leaking its wording.
                let message = match &diagnostic.message {
                    Some(text) => format!("syntax error: {text}"),
                    None => "syntax error".to_string(),
                };
                report
                    .error(message)
                    .at(span_from(
                        diagnostic.span.offset(),
                        diagnostic.span.len(),
                        id,
                    ))
                    .emit();
            }
            None
        }
    }
}

/// Parses one registered source into a `T`, pushing syntax errors and
/// structural problems into the report.
pub fn parse_kdl<T: FromFields>(
    sources: &SourceMap,
    id: SourceId,
    report: &mut Report,
) -> Option<T> {
    let fields = parse_kdl_fields(sources, id, report)?;
    T::from_fields(&fields, report)
}

/// Converts a kdl-rs offset and length to a confval [`Span`]. A zero-length
/// span, which a diagnostic can carry, widens to one byte so it stays visible
/// when rendered.
fn span_from(offset: usize, len: usize, source: SourceId) -> Span {
    let start = offset as u32;
    let end = if len == 0 {
        start.saturating_add(1)
    } else {
        start.saturating_add(len as u32)
    };
    Span::new(source, start, end)
}

/// The span of a kdl-rs node, entry, identifier, or document. The macro takes
/// the spanned value rather than the span, because the span type belongs to
/// miette, which is not a direct dependency, and a macro needs no name for it.
macro_rules! span_of {
    ($spanned:expr, $source:expr) => {{
        let span = $spanned.span();
        span_from(span.offset(), span.len(), $source)
    }};
}

/// Normalizes a document's nodes into neutral fields. `enclosing` is the span
/// missing-field errors point at: the children block, or the whole file at the
/// root.
fn fields_of_document(
    document: &KdlDocument,
    enclosing: Span,
    source: SourceId,
    report: &mut Report,
) -> Fields {
    let items = document
        .nodes()
        .iter()
        .map(|node| field_of_node(node, source, report))
        .collect();
    Fields::new(source, enclosing, items)
}

/// Maps one node to one field by its shape: only arguments make a value, and
/// properties or children make a block.
fn field_of_node(node: &KdlNode, source: SourceId, report: &mut Report) -> Field {
    let name_span = span_of!(node.name(), source);
    let node_span = span_of!(node, source);
    let (properties, arguments): (Vec<&KdlEntry>, Vec<&KdlEntry>) = node
        .entries()
        .iter()
        .partition(|entry| entry.name().is_some());
    let kind = if properties.is_empty() && node.children().is_none() {
        FieldKind::Value(value_of_arguments(&arguments, node_span, source))
    } else {
        // The model has no field for a block label, so an argument on a block
        // node, the `service "web" { ... }` idiom, is unrepresentable. Report
        // it once and keep the block, so later errors are still collected.
        if let Some(first) = arguments.first() {
            report
                .error("arguments are not supported on a node with properties or children")
                .at(span_of!(first, source))
                .emit();
        }
        let mut items: Vec<Field> = properties
            .iter()
            .filter_map(|entry| field_of_property(entry, source))
            .collect();
        let enclosing = match node.children() {
            Some(children) => {
                items.extend(
                    children
                        .nodes()
                        .iter()
                        .map(|child| field_of_node(child, source, report)),
                );
                span_of!(children, source)
            }
            // A properties-only block has no children document to take a span
            // from, so its level spans the node.
            None => node_span,
        };
        FieldKind::Block(Fields::new(source, enclosing, items))
    };
    Field::parsed(node.name().value(), name_span, node_span, source, kind)
}

/// The value of a node that carries only arguments: a bare node is an empty
/// sequence, one argument is its scalar, and more are a sequence. The sequence
/// spans the node, and a single scalar spans its entry.
fn value_of_arguments(arguments: &[&KdlEntry], node_span: Span, source: SourceId) -> Value {
    match arguments {
        [] => Value {
            span: node_span,
            kind: ValueKind::Seq(Vec::new()),
        },
        [only] => value_of_entry(only, source),
        _ => Value {
            span: node_span,
            kind: ValueKind::Seq(
                arguments
                    .iter()
                    .map(|entry| value_of_entry(entry, source))
                    .collect(),
            ),
        },
    }
}

/// Maps a property entry to a leaf field. The name span is the property name's
/// identifier and the field span is the whole entry, name and value together,
/// which is what the other frontends report. The value inside it carries the
/// narrower span [`value_span_of`] computes.
fn field_of_property(entry: &KdlEntry, source: SourceId) -> Option<Field> {
    let name: &KdlIdentifier = entry.name()?;
    let entry_span = span_of!(entry, source);
    Some(Field::parsed(
        name.value(),
        span_of!(name, source),
        entry_span,
        source,
        FieldKind::Value(value_of_entry(entry, source)),
    ))
}

/// The span of an entry's value alone.
///
/// kdl-rs spans an entry from its name through its value, so a property's span
/// covers `cert="a.pem"` while a diagnostic should underline `"a.pem"`. The
/// value's own text is the tail of that span, so its length locates the start.
/// This holds through whitespace around the `=`, a quoted key, an `=` inside
/// the value, and a type annotation, because each of those sits ahead of the
/// value text.
///
/// An argument entry has no name, so its span is already the value alone and
/// the arithmetic returns it unchanged. An entry built rather than parsed
/// carries no format to measure, so it keeps the whole span.
fn value_span_of(entry: &KdlEntry, source: SourceId) -> Span {
    let span = span_of!(entry, source);
    let Some(format) = entry.format() else {
        return span;
    };
    let start = span.end.saturating_sub(format.value_repr.len() as u32);
    Span::new(source, start.max(span.start), span.end)
}

/// Converts one entry's value into a neutral [`Value`]. Anything the model has
/// no scalar for, `#null` and an integer beyond `i64`, becomes
/// [`ValueKind::Other`] with a diagnostic label.
fn value_of_entry(entry: &KdlEntry, source: SourceId) -> Value {
    let span = value_span_of(entry, source);
    let kind = match entry.value() {
        KdlValue::String(string) => ValueKind::Scalar(Scalar::String(string.clone())),
        KdlValue::Integer(int) => match i64::try_from(*int) {
            Ok(int) => ValueKind::Scalar(Scalar::Int(int)),
            Err(_) => ValueKind::Other("oversized integer"),
        },
        KdlValue::Float(float) => ValueKind::Scalar(Scalar::Float(*float)),
        KdlValue::Bool(boolean) => ValueKind::Scalar(Scalar::Bool(*boolean)),
        KdlValue::Null => ValueKind::Other("null"),
    };
    Value { span, kind }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::parse::{
        parse_bool_field, parse_float_field, parse_int_field, parse_string_field,
        parse_string_list_field, parse_struct_field,
    };
    use crate::source::Located;

    struct Probe;
    impl FromFields for Probe {
        fn from_fields(_: &Fields, _: &mut Report) -> Option<Self> {
            Some(Probe)
        }
    }

    fn parse(input: &str) -> (SourceId, Fields) {
        let mut sources = SourceMap::new();
        let id = sources.add("test.kdl", input);
        let mut report = Report::new();
        let fields = parse_kdl_fields(&sources, id, &mut report).unwrap();
        assert!(
            !report.has_issues(),
            "frontend reported: {:?}",
            report.issues()
        );
        (id, fields)
    }

    #[test]
    fn scalar_arguments_parse_with_entry_spans() {
        // Arrange
        let input = "hostname \"example.com\"\nport 8080\ndaemon #true\nratio 0.5\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        let hostname = parse_string_field(fields.get("hostname").unwrap(), &mut report).unwrap();
        assert_eq!(hostname.value, "example.com");
        assert_eq!(
            &input[hostname.span.start as usize..hostname.span.end as usize],
            "\"example.com\""
        );
        let port = parse_int_field(fields.get("port").unwrap(), &mut report).unwrap();
        assert_eq!(port.value, 8080);
        assert!(
            parse_bool_field(fields.get("daemon").unwrap(), &mut report)
                .unwrap()
                .value
        );
        let ratio = parse_float_field(fields.get("ratio").unwrap(), &mut report).unwrap();
        assert_eq!(ratio.value, 0.5);
        assert!(!report.has_issues());
    }

    #[test]
    fn identifier_strings_parse_like_quoted_strings() {
        // Arrange
        // KDL 2.0 allows an identifier spelling for a string value.
        let input = "mode enforce\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        let mode = parse_string_field(fields.get("mode").unwrap(), &mut report).unwrap();
        assert_eq!(mode.value, "enforce");
    }

    #[test]
    fn repeated_arguments_parse_as_a_list_with_element_spans() {
        // Arrange
        let input = "allow \"10.0.0.0/8\" \"192.168.0.0/16\"\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        let list = parse_string_list_field(fields.get("allow").unwrap(), &mut report).unwrap();
        assert_eq!(list.value.len(), 2);
        let first = &list.value[0];
        assert_eq!(first.value, "10.0.0.0/8");
        assert_eq!(
            &input[first.span.start as usize..first.span.end as usize],
            "\"10.0.0.0/8\""
        );
    }

    #[test]
    fn a_bare_node_is_an_empty_sequence() {
        // Arrange
        let input = "allow\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        let list = parse_string_list_field(fields.get("allow").unwrap(), &mut report).unwrap();
        assert!(list.value.is_empty());
        assert!(!report.has_issues());
    }

    #[test]
    fn a_bare_node_where_a_scalar_is_expected_reports_found_array() {
        // Arrange
        // The natural typo of a forgotten value maps to an empty sequence, so
        // the operator-visible text names an array.
        let input = "hostname\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_string_field(fields.get("hostname").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected string, found array");
    }

    #[test]
    fn an_empty_children_block_is_an_empty_block() {
        // Arrange
        let input = "tls {\n}\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let FieldKind::Block(inner) = &fields.get("tls").unwrap().kind else {
            panic!("tls should be a block");
        };
        assert_eq!(inner.iter().count(), 0);
    }

    #[test]
    fn a_property_value_span_covers_the_value_alone() {
        // Arrange
        // Each property puts something different between the name and the
        // value: padding around the `=`, an `=` inside the value, a quoted key,
        // and a type annotation.
        let input = "tls plain=\"a.pem\" padded = 8443 inner=\"a=b\" \"odd key\"=1 typed=(u8)7\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let FieldKind::Block(inner) = &fields.get("tls").unwrap().kind else {
            panic!("tls should be a block");
        };
        let spelled = |name: &str| {
            let FieldKind::Value(value) = &inner.get(name).unwrap().kind else {
                panic!("{name} should be a value");
            };
            &input[value.span.start as usize..value.span.end as usize]
        };
        assert_eq!(spelled("plain"), "\"a.pem\"");
        assert_eq!(spelled("padded"), "8443");
        assert_eq!(spelled("inner"), "\"a=b\"");
        assert_eq!(spelled("odd key"), "1");
        assert_eq!(spelled("typed"), "7");
    }

    #[test]
    fn an_argument_value_span_is_unchanged_by_the_narrowing() {
        // Arrange
        let input = "port 8443\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        // An argument entry has no name, so its span is already the value alone.
        let FieldKind::Value(value) = &fields.get("port").unwrap().kind else {
            panic!("port should be a value");
        };
        assert_eq!(
            &input[value.span.start as usize..value.span.end as usize],
            "8443"
        );
    }

    #[test]
    fn properties_parse_as_a_block() {
        // Arrange
        let input = "tls cert=\"a.pem\" key=\"k.pem\"\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        assert!(parsed.is_some());
        let FieldKind::Block(inner) = &fields.get("tls").unwrap().kind else {
            panic!("tls should be a block");
        };
        let cert = parse_string_field(inner.get("cert").unwrap(), &mut report).unwrap();
        assert_eq!(cert.value, "a.pem");
        // The property value span covers the value alone, so a diagnostic
        // underlines what the operator would change, the way TOML and HCL do.
        assert_eq!(
            &input[cert.span.start as usize..cert.span.end as usize],
            "\"a.pem\""
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn properties_and_children_merge_into_one_block() {
        // Arrange
        let input = "tls cert=\"a.pem\" {\n  key \"k.pem\"\n}\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let FieldKind::Block(inner) = &fields.get("tls").unwrap().kind else {
            panic!("tls should be a block");
        };
        let names: Vec<&str> = inner.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["cert", "key"]);
    }

    #[test]
    fn an_argument_beside_children_reports_and_keeps_the_block() {
        // Arrange
        // The label idiom has no home in the model, which carries no block
        // labels.
        let input = "service \"web\" {\n  port 8080\n}\n";
        let mut sources = SourceMap::new();
        let id = sources.add("test.kdl", input);
        let mut report = Report::new();

        // Act
        let fields = parse_kdl_fields(&sources, id, &mut report).unwrap();

        // Assert
        assert_eq!(report.issues().len(), 1);
        assert_eq!(
            report.issues()[0].message,
            "arguments are not supported on a node with properties or children"
        );
        let issue_span = report.issues()[0].span.unwrap();
        assert_eq!(
            &input[issue_span.start as usize..issue_span.end as usize],
            "\"web\""
        );
        let FieldKind::Block(inner) = &fields.get("service").unwrap().kind else {
            panic!("service should still be a block");
        };
        assert!(inner.get("port").is_some());
    }

    #[test]
    fn null_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        let input = "pid_file #null\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_string_field(fields.get("pid_file").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected string, found null");
    }

    #[test]
    fn an_oversized_integer_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        // i128 holds this, i64 does not.
        let input = "offset 9223372036854775808\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_int_field(fields.get("offset").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected integer, found oversized integer"
        );
    }

    #[test]
    fn a_type_annotation_is_read_through() {
        // Arrange
        let input = "port (u16)8080\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let mut report = Report::new();
        let port = parse_int_field(fields.get("port").unwrap(), &mut report).unwrap();
        assert_eq!(port.value, 8080);
        assert!(!report.has_issues());
    }

    #[test]
    fn a_v1_only_document_is_a_syntax_error() {
        // Arrange
        // KDL 1.0 spells null bare. 2.0 requires #null, and the frontend
        // parses 2.0 alone with no fallback.
        let input = "pid_file null\n";
        let mut sources = SourceMap::new();
        let id = sources.add("v1.kdl", input);
        let mut report = Report::new();

        // Act
        let parsed = parse_kdl_fields(&sources, id, &mut report);

        // Assert
        assert!(parsed.is_none());
        assert!(report.has_errors());
        assert!(
            report.issues()[0].message.starts_with("syntax error"),
            "got: {}",
            report.issues()[0].message
        );
    }

    #[test]
    fn syntax_error_diagnostics_each_carry_a_span() {
        // Arrange
        let input = "port = 8080\n";
        let mut sources = SourceMap::new();
        let id = sources.add("broken.kdl", input);
        let mut report = Report::new();

        // Act
        let parsed = parse_kdl_fields(&sources, id, &mut report);

        // Assert
        assert!(parsed.is_none());
        assert!(report.has_errors());
        for issue in report.issues() {
            assert!(issue.message.starts_with("syntax error"), "got: {issue:?}");
            assert!(issue.span.is_some(), "diagnostic without a span: {issue:?}");
        }
    }

    #[test]
    fn nested_blocks_carry_the_children_span_as_enclosing() {
        // Arrange
        let input = "server {\n  tls {\n    cert \"a.pem\"\n  }\n}\n";

        // Act
        let (_, fields) = parse(input);

        // Assert
        let FieldKind::Block(server) = &fields.get("server").unwrap().kind else {
            panic!("server should be a block");
        };
        let FieldKind::Block(tls) = &server.get("tls").unwrap().kind else {
            panic!("tls should be a block");
        };
        // The enclosing span sits inside the children block, so a
        // missing-field error points into the braces.
        let enclosing = tls.enclosing();
        assert!(enclosing.start as usize > input.find("tls").unwrap());
        assert!((enclosing.end as usize) <= input.len());
    }
}
