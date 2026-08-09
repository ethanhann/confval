//! YAML frontend: parses YAML 1.2 text into the format-neutral [`Fields`] tree.
//!
//! This module's whole job is the conversion from saphyr-parser's event stream
//! to the owned, format-neutral model in [`field`](crate::format::field). Once
//! [`parse_yaml`] hands back a `Fields`, every span has been captured and no
//! saphyr-parser type escapes. The leaf parsers, the derive-generated walks,
//! and the handwritten [`FromFields`] impls all work against the neutral model.
//!
//! The core schema resolution lives in the sibling `resolve` module, and the
//! write path, [`emit_yaml`], in the sibling `emit` module.
//!
//! The frontend drives the parser's pull API rather than loading a document
//! model. A loader stores a mapping in a hash map, which drops one value of a
//! duplicated key before the frontend can see it. The event stream hands over
//! every entry in order, so duplicates reach the model and the generated walk
//! resolves them by the spec's declared shape. The stream also makes aliases,
//! tags, and extra documents visible, so each has a decided behavior below.
//!
//! A configuration is named fields, so the document root must be a mapping.
//! Below the root every mapping is a [`ValueKind::Map`], in block or flow
//! style, so [`FieldKind::Block`] never arises from a YAML parse.
//!
//! Behavior contract:
//!
//! - A syntax error is reported as one issue at its position, and parsing
//!   returns `None`.
//! - A root that is not a mapping, and a document with no root node, each
//!   report `expected a mapping at the document root` and return `None`.
//! - A second document reports `expected a single document` and returns `None`,
//!   so a configuration cannot lose its tail to a silent discard.
//! - A plain scalar resolves through the YAML 1.2 core schema. A quoted,
//!   literal, or folded scalar is a string whatever its text.
//! - Values outside the neutral model (`null`, an integer beyond `i64`, a float
//!   that overflows `f64`, an alias, a tag the frontend refuses) become
//!   [`ValueKind::Other`] carrying a diagnostic label, so they surface as
//!   ordinary type mismatches at the field that used them.
//! - A non-scalar key has no field name the model can hold. It reports
//!   `expected a scalar key` and the entry is skipped, so one exotic key does
//!   not hide the errors after it.
//! - Duplicate keys stay separate fields, the way they do in JSON.

use crate::diagnostic::Report;
use crate::format::field::{Field, FieldKind, Fields, FromFields, Scalar, Value, ValueKind};
use crate::source::{SourceId, SourceMap, Span};
use resolve::Core;
use saphyr_parser::{Event, Parser, ScalarStyle, Span as YamlSpan, StrInput, Tag};

mod emit;
mod resolve;
pub use emit::emit_yaml;

/// The message a root that cannot hold named fields reports.
const ROOT_MUST_BE_A_MAPPING: &str = "expected a mapping at the document root";
/// The message a stream carrying more than one document reports.
const ONE_DOCUMENT: &str = "expected a single document";
/// The message a key the model has no name for reports.
const SCALAR_KEY: &str = "expected a scalar key";
/// The label every tag the frontend refuses carries.
const TAGGED: &str = "tagged value";

/// Parses one registered source into the neutral [`Fields`] tree.
///
/// When you assemble configuration from several sources, you hold the returned
/// `Fields`, merge it with the others, and run [`FromFields`] once on the
/// merged result. A syntax error, a root that is not a mapping, and a second
/// document are the three failures that yield no tree. Each is reported and
/// returns `None`. Field-level problems are reported but do not stop the parse,
/// so a tree that parsed still reaches validation.
pub fn parse_yaml_fields(sources: &SourceMap, id: SourceId, report: &mut Report) -> Option<Fields> {
    let Some(source) = sources.get(id) else {
        report
            .error("internal error: parse_yaml_fields called with an unregistered source id")
            .emit();
        return None;
    };
    let document = Span::new(id, 0, source.text.len() as u32);
    Reader {
        parser: Parser::new_from_str(&source.text),
        offsets: Offsets::new(&source.text),
        source: id,
        report,
    }
    .document(document)
}

/// Parses one registered source into a `T`, pushing syntax errors and
/// structural problems into the report.
pub fn parse_yaml<T: FromFields>(
    sources: &SourceMap,
    id: SourceId,
    report: &mut Report,
) -> Option<T> {
    let fields = parse_yaml_fields(sources, id, report)?;
    T::from_fields(&fields, report)
}

/// The byte offset of each character in the source.
///
/// saphyr-parser's `Marker` carries a character index, and a confval [`Span`]
/// carries a byte offset, so every marker converts through this table. An
/// all-ASCII source needs none, because the two indices agree there, which is
/// the common case for a configuration file.
struct Offsets {
    positions: Option<Vec<u32>>,
    len: u32,
}

impl Offsets {
    fn new(text: &str) -> Self {
        let len = text.len() as u32;
        if text.is_ascii() {
            return Self {
                positions: None,
                len,
            };
        }
        let mut positions: Vec<u32> = text.char_indices().map(|(at, _)| at as u32).collect();
        positions.push(len);
        Self {
            positions: Some(positions),
            len,
        }
    }

    /// The byte offset of one character index, clamped to the source's end.
    fn at(&self, characters: usize) -> u32 {
        match &self.positions {
            None => (characters as u32).min(self.len),
            Some(positions) => positions.get(characters).copied().unwrap_or(self.len),
        }
    }
}

/// The pull loop over one document's events.
struct Reader<'input, 'report> {
    parser: Parser<'input, StrInput<'input>>,
    offsets: Offsets,
    source: SourceId,
    report: &'report mut Report,
}

impl<'input> Reader<'input, '_> {
    /// The next event, or `None` once the stream has failed. A scan error and
    /// an exhausted stream are both reported here, so a `None` anywhere below
    /// has already pushed its issue.
    fn next(&mut self) -> Option<(Event<'input>, YamlSpan)> {
        match self.parser.next_event() {
            Some(Ok(step)) => Some(step),
            Some(Err(error)) => {
                let at = self.offsets.at(error.marker().index());
                self.report
                    .error(format!("syntax error: {}", lead_lowercase(error.info())))
                    .at(widen(Span::new(self.source, at, at)))
                    .emit();
                None
            }
            // Unreachable for a well-formed stream, which always closes with
            // `StreamEnd`. Reported rather than silent, so a `None` return
            // always carries an issue.
            None => {
                self.report
                    .error("syntax error: unexpected end of input")
                    .emit();
                None
            }
        }
    }

    /// Reads the one document a configuration file holds.
    fn document(&mut self, document: Span) -> Option<Fields> {
        loop {
            let (event, _) = self.next()?;
            match event {
                Event::DocumentStart(_) => break,
                // An empty file, a whitespace-only file, and a file holding
                // only comments all reach the end with no document.
                Event::StreamEnd => {
                    self.report
                        .error(ROOT_MUST_BE_A_MAPPING)
                        .at(document)
                        .emit();
                    return None;
                }
                _ => continue,
            }
        }
        let (event, span) = self.next()?;
        // The root check turns on the node's kind, so a tag on the root mapping
        // is read through.
        let Event::MappingStart(..) = event else {
            let at = self.span(span);
            self.report.error(ROOT_MUST_BE_A_MAPPING).at(at).emit();
            return None;
        };
        let (items, _) = self.entries(span)?;
        let fields = Fields::new(self.source, document, items);
        loop {
            let (event, span) = self.next()?;
            match event {
                Event::StreamEnd => return Some(fields),
                Event::DocumentStart(_) => {
                    let at = self.span(span);
                    self.report.error(ONE_DOCUMENT).at(at).emit();
                    return None;
                }
                _ => continue,
            }
        }
    }

    /// Reads one mapping's entries, and the span running from its opening event
    /// through its closing one. The span is what a nested level reports a
    /// missing field at.
    fn entries(&mut self, start: YamlSpan) -> Option<(Vec<Field>, Span)> {
        let mut items: Vec<Field> = Vec::new();
        loop {
            let (event, key_span) = self.next()?;
            let name = match event {
                Event::MappingEnd => return Some((items, self.range(start, key_span))),
                // A key is a name, not a typed value, so its text is the name
                // whatever the schema would resolve it to.
                Event::Scalar(text, ..) => text.into_owned(),
                other => {
                    let at = self.span(key_span);
                    self.report.error(SCALAR_KEY).at(at).emit();
                    self.drain(opens(&other), key_span)?;
                    let (value, value_span) = self.next()?;
                    self.drain(opens(&value), value_span)?;
                    continue;
                }
            };
            let name_span = self.span(key_span);
            let (event, value_span) = self.next()?;
            let value = self.node(event, value_span)?;
            let field_span = Span::new(self.source, name_span.start, value.span.end);
            items.push(Field::parsed(
                name,
                name_span,
                field_span,
                self.source,
                FieldKind::Value(value),
            ));
        }
    }

    /// Reads one sequence's elements and its whole span.
    fn elements(&mut self, start: YamlSpan) -> Option<(Vec<Value>, Span)> {
        let mut elements: Vec<Value> = Vec::new();
        loop {
            let (event, span) = self.next()?;
            if matches!(event, Event::SequenceEnd) {
                return Some((elements, self.range(start, span)));
            }
            elements.push(self.node(event, span)?);
        }
    }

    /// Reads one node, whose opening event has already been taken.
    fn node(&mut self, event: Event<'input>, span: YamlSpan) -> Option<Value> {
        match event {
            Event::Scalar(text, style, _, tag) => Some(Value {
                span: self.span(span),
                kind: scalar_kind(&text, style, tag.as_deref()),
            }),
            // An alias is not expanded. It surfaces once, at its own position,
            // as an ordinary type mismatch.
            Event::Alias(_) => Some(Value {
                span: self.span(span),
                kind: ValueKind::Other("alias"),
            }),
            Event::SequenceStart(_, tag) => {
                if reads_through(tag.as_deref(), "seq") {
                    let (elements, whole) = self.elements(span)?;
                    return Some(Value {
                        span: whole,
                        kind: ValueKind::Seq(elements),
                    });
                }
                self.refuse(span)
            }
            Event::MappingStart(_, tag) => {
                if reads_through(tag.as_deref(), "map") {
                    let (items, whole) = self.entries(span)?;
                    return Some(Value {
                        span: whole,
                        kind: ValueKind::Map(Fields::new(self.source, whole, items)),
                    });
                }
                self.refuse(span)
            }
            // Unreachable: the parser opens every node with one of the arms
            // above. Reported rather than silent, for the same reason `next`
            // reports an exhausted stream.
            _ => {
                let at = self.span(span);
                self.report
                    .error("internal error: unexpected YAML event in a value position")
                    .at(at)
                    .emit();
                None
            }
        }
    }

    /// Consumes a collection carrying a tag the frontend refuses, so parsing
    /// continues past it, and yields the label its field will report. The
    /// opening event has already been taken, so the depth starts at one.
    fn refuse(&mut self, span: YamlSpan) -> Option<Value> {
        let end = self.drain(1, span)?;
        Some(Value {
            span: self.range(span, end),
            kind: ValueKind::Other(TAGGED),
        })
    }

    /// Consumes the events of a node whose opening event was already taken, and
    /// returns the closing event's span. `depth` is one for a collection and
    /// zero for a scalar or an alias, which close themselves.
    fn drain(&mut self, mut depth: u32, opening_span: YamlSpan) -> Option<YamlSpan> {
        if depth == 0 {
            return Some(opening_span);
        }
        loop {
            let (event, span) = self.next()?;
            match event {
                Event::SequenceStart(..) | Event::MappingStart(..) => depth += 1,
                Event::SequenceEnd | Event::MappingEnd => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(span);
                    }
                }
                _ => {}
            }
        }
    }

    /// One event's span.
    fn span(&self, span: YamlSpan) -> Span {
        self.range(span, span)
    }

    /// The span running from one event's start to another's end.
    fn range(&self, start: YamlSpan, end: YamlSpan) -> Span {
        let from = self.offsets.at(start.start.index());
        let to = self.offsets.at(end.end.index()).max(from);
        widen(Span::new(self.source, from, to))
    }
}

/// The depth one event opens: one for a collection, zero for anything that
/// closes itself.
fn opens(event: &Event<'_>) -> u32 {
    u32::from(matches!(
        event,
        Event::SequenceStart(..) | Event::MappingStart(..)
    ))
}

/// Widens a zero-width span to one byte, so it stays visible when rendered. A
/// valueless key, `key:`, reads as a null whose scalar has no extent.
fn widen(span: Span) -> Span {
    if span.end > span.start {
        return span;
    }
    Span::new(span.source, span.start, span.start.saturating_add(1))
}

/// A scan error's message with its first character lowercased, so it reads as a
/// continuation of `syntax error:`. saphyr-parser writes lowercase messages
/// today, and this keeps an upgrade that changes one from regressing the prefix.
fn lead_lowercase(message: &str) -> String {
    let mut characters = message.chars();
    match characters.next() {
        Some(first) => first.to_lowercase().chain(characters).collect(),
        None => message.to_string(),
    }
}

/// Whether a collection's tag leaves the node reading as itself.
///
/// An absent tag and the non-specific `!` both do. So does the core schema tag
/// for the node's own kind, which restates what the shape already says. Any
/// other tag is refused, because the model has no place to put it.
fn reads_through(tag: Option<&Tag>, kind: &str) -> bool {
    match tag {
        None => true,
        Some(tag) if non_specific(tag) => true,
        Some(tag) => tag.is_yaml_core_schema() && tag.suffix == kind,
    }
}

/// Whether a tag is YAML's non-specific `!`, which the schema resolves by node
/// kind: a string on a scalar, and the node itself on a collection.
fn non_specific(tag: &Tag) -> bool {
    tag.handle.is_empty() && tag.suffix == "!"
}

/// One scalar's neutral value kind, from its text, its style, and its tag.
fn scalar_kind(text: &str, style: ScalarStyle, tag: Option<&Tag>) -> ValueKind {
    let Some(tag) = tag else {
        return match style {
            ScalarStyle::Plain => of_core(resolve::resolve(text), text),
            // A quoted, literal, or folded scalar is a string whatever its
            // text, so `port: "8080"` is the string `8080`.
            _ => string(text),
        };
    };
    if non_specific(tag) {
        return string(text);
    }
    if !tag.is_yaml_core_schema() {
        return ValueKind::Other(TAGGED);
    }
    match tag.suffix.as_str() {
        "str" => string(text),
        suffix @ ("null" | "bool" | "int" | "float") => match resolve::resolve_as(suffix, text) {
            Some(core) => of_core(core, text),
            None => ValueKind::Other(TAGGED),
        },
        // A core tag naming a collection sits on the wrong node kind here, and
        // an unknown `!!name` has no reading at all.
        _ => ValueKind::Other(TAGGED),
    }
}

/// The neutral value kind for one resolved scalar.
fn of_core(core: Core, text: &str) -> ValueKind {
    match core {
        Core::Null => ValueKind::Other("null"),
        Core::Bool(value) => ValueKind::Scalar(Scalar::Bool(value)),
        Core::Int(value) => ValueKind::Scalar(Scalar::Int(value)),
        Core::Float(value) => ValueKind::Scalar(Scalar::Float(value)),
        Core::OversizedInt => ValueKind::Other("oversized integer"),
        Core::OversizedFloat => ValueKind::Other("oversized number"),
        Core::Str => string(text),
    }
}

fn string(text: &str) -> ValueKind {
    ValueKind::Scalar(Scalar::String(text.to_string()))
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

    fn parse(input: &str) -> Fields {
        let mut sources = SourceMap::new();
        let id = sources.add("test.yaml", input);
        let mut report = Report::new();
        let fields = parse_yaml_fields(&sources, id, &mut report)
            .unwrap_or_else(|| panic!("{input:?} should parse, got: {:?}", report.issues()));
        assert!(
            !report.has_issues(),
            "frontend reported: {:?}",
            report.issues()
        );
        fields
    }

    fn reject(input: &str) -> Report {
        let mut sources = SourceMap::new();
        let id = sources.add("test.yaml", input);
        let mut report = Report::new();

        assert!(parse_yaml_fields(&sources, id, &mut report).is_none());
        report
    }

    fn kind_of<'f>(fields: &'f Fields, name: &str) -> &'f ValueKind {
        let FieldKind::Value(value) = &fields.get(name).unwrap().kind else {
            panic!("{name} should be an attribute value");
        };
        &value.kind
    }

    #[test]
    fn scalars_parse_with_value_spans() {
        // Arrange
        let input = "hostname: \"example.com\"\nport: 8080\ndaemon: false\nratio: 0.5\n";

        // Act
        let fields = parse(input);

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
            !parse_bool_field(fields.get("daemon").unwrap(), &mut report)
                .unwrap()
                .value
        );
        let ratio = parse_float_field(fields.get("ratio").unwrap(), &mut report).unwrap();
        assert_eq!(ratio.value, 0.5);
        assert!(!report.has_issues());
    }

    #[test]
    fn a_quoted_scalar_is_a_string_whatever_its_text() {
        // Arrange
        // The quoting decides, not the schema, so an integer field reports the
        // mismatch rather than reading the digits.
        let input = "port: \"8080\"\ncountry: no\nliteral: |\n  text\n";

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_int_field(fields.get("port").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected integer, found string");
        let mut report = Report::new();
        // The 1.2 core schema drops the 1.1 booleans, so `no` is text.
        assert_eq!(
            parse_string_field(fields.get("country").unwrap(), &mut report)
                .unwrap()
                .value,
            "no"
        );
        assert_eq!(
            parse_string_field(fields.get("literal").unwrap(), &mut report)
                .unwrap()
                .value,
            "text\n"
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn both_mapping_styles_parse_to_the_same_shape() {
        // Arrange
        let block = "tls:\n  cert: \"a.pem\"\n  key: \"k.pem\"\n";
        let flow = "tls: {cert: \"a.pem\", key: \"k.pem\"}\n";

        // Act
        let (from_block, from_flow) = (parse(block), parse(flow));

        // Assert
        let mut report = Report::new();
        for fields in [&from_block, &from_flow] {
            let parsed: Option<Located<Probe>> =
                parse_struct_field(fields.get("tls").unwrap(), &mut report);
            assert!(parsed.is_some());
            let ValueKind::Map(inner) = kind_of(fields, "tls") else {
                panic!("a nested mapping should be a map, never a block");
            };
            assert_eq!(
                parse_string_field(inner.get("cert").unwrap(), &mut report)
                    .unwrap()
                    .value,
                "a.pem"
            );
        }
        assert!(!report.has_issues());
    }

    #[test]
    fn a_nested_mapping_carries_its_own_span_as_enclosing() {
        // Arrange
        let input = "tls:\n  cert: \"a.pem\"\nport: 1\n";

        // Act
        let fields = parse(input);

        // Assert
        let ValueKind::Map(inner) = kind_of(&fields, "tls") else {
            panic!("tls should be a map");
        };
        // A missing-field error inside `tls` points at the block's entries
        // rather than at the whole document.
        let text = &input[inner.enclosing().start as usize..inner.enclosing().end as usize];
        assert!(text.contains("cert"), "got: {text:?}");
        assert!(!text.contains("tls:"), "got: {text:?}");
    }

    #[test]
    fn sequences_parse_in_both_styles_with_element_spans() {
        // Arrange
        let input =
            "flow: [\"10.0.0.0/8\", \"192.168.0.0/16\"]\nblock:\n  - \"a\"\n  - \"b\"\nempty: []\n";

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let flow = parse_string_list_field(fields.get("flow").unwrap(), &mut report).unwrap();
        assert_eq!(flow.value.len(), 2);
        assert_eq!(
            &input[flow.value[0].span.start as usize..flow.value[0].span.end as usize],
            "\"10.0.0.0/8\""
        );
        let block = parse_string_list_field(fields.get("block").unwrap(), &mut report).unwrap();
        assert_eq!(block.value.len(), 2);
        assert!(
            parse_string_list_field(fields.get("empty").unwrap(), &mut report)
                .unwrap()
                .value
                .is_empty()
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn a_key_span_covers_the_key_and_the_field_span_reaches_the_value() {
        // Arrange
        let input = "port: 8080\n";

        // Act
        let fields = parse(input);

        // Assert
        let field = fields.get("port").unwrap();
        assert_eq!(
            &input[field.name_span.start as usize..field.name_span.end as usize],
            "port"
        );
        assert_eq!(
            &input[field.span.start as usize..field.span.end as usize],
            "port: 8080"
        );
    }

    #[test]
    fn null_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        // Every null form reaches the same label, including the valueless key.
        let input = "a: null\nb: ~\nc:\nd: NULL\n";

        // Act
        let fields = parse(input);

        // Assert
        for name in ["a", "b", "c", "d"] {
            let mut report = Report::new();
            assert!(parse_string_field(fields.get(name).unwrap(), &mut report).is_none());
            assert_eq!(
                report.issues()[0].message,
                "expected string, found null",
                "field {name}"
            );
        }
    }

    #[test]
    fn a_valueless_key_widens_its_zero_width_span() {
        // Arrange
        // `key:` reads as a null whose scalar has no extent, and a zero-width
        // span renders as no highlight at all.
        let input = "key:\nport: 1\n";

        // Act
        let fields = parse(input);

        // Assert
        let FieldKind::Value(value) = &fields.get("key").unwrap().kind else {
            panic!("key should be an attribute value");
        };
        assert_eq!(value.span.end, value.span.start + 1);
    }

    #[test]
    fn an_oversized_number_surfaces_as_a_type_mismatch_on_access() {
        // Arrange
        let input = "offset: 9223372036854775808\nratio: 1e999\n";

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        assert!(parse_int_field(fields.get("offset").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected integer, found oversized integer"
        );
        let mut report = Report::new();
        assert!(parse_float_field(fields.get("ratio").unwrap(), &mut report).is_none());
        assert_eq!(
            report.issues()[0].message,
            "expected number, found oversized number"
        );
    }

    #[test]
    fn a_scalar_where_a_nested_spec_is_expected_reports_the_shared_wording() {
        // Arrange
        // The expected side keeps the shared parsers' noun, so the message
        // names a block even though YAML has no blocks.
        let input = "tls: \"a.pem\"\n";

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let parsed: Option<Located<Probe>> =
            parse_struct_field(fields.get("tls").unwrap(), &mut report);
        assert!(parsed.is_none());
        assert_eq!(report.issues()[0].message, "expected block, found string");
    }

    #[test]
    fn duplicate_keys_stay_separate_fields_in_document_order() {
        // Arrange
        let input = "allow: \"a\"\nport: 1\nallow: \"b\"\n";

        // Act
        let fields = parse(input);

        // Assert
        let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["allow", "port", "allow"]);
    }

    #[test]
    fn a_scalar_key_reads_as_its_text_whatever_it_resolves_to() {
        // Arrange
        // A name is text, not a typed value, so the schema never touches a key.
        let input = "8080: \"a\"\ntrue: \"b\"\nnull: \"c\"\n";

        // Act
        let fields = parse(input);

        // Assert
        let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["8080", "true", "null"]);
    }

    #[test]
    fn escapes_decode_and_later_spans_stay_byte_accurate() {
        // Arrange
        // A short escape, a four-digit escape, and a literal multibyte
        // character. saphyr-parser counts characters and confval counts bytes,
        // so a span past this text drifts unless the frontend converts.
        let input = "greeting: \"a\\nb\\u00e9\"\ncost: \"€\"\nport: 8080\n";

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        let greeting = parse_string_field(fields.get("greeting").unwrap(), &mut report).unwrap();
        assert_eq!(greeting.value, "a\nb\u{e9}");
        let port = fields.get("port").unwrap();
        assert_eq!(
            &input[port.span.start as usize..port.span.end as usize],
            "port: 8080"
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn a_root_that_is_not_a_mapping_reports_and_yields_no_tree() {
        // Arrange
        // A root alias is not among these. The root is the first node, so an
        // anchor it could refer to cannot have been defined yet, and the
        // parser rejects it as an unknown anchor before the root check runs.
        for input in ["- 1\n- 2\n", "just text\n", "8080\n", "[a, b]\n"] {
            // Act
            let report = reject(input);

            // Assert
            assert_eq!(
                report.issues()[0].message,
                ROOT_MUST_BE_A_MAPPING,
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn a_document_with_no_root_node_reports_at_the_whole_document() {
        // Arrange
        // An empty file, whitespace alone, and comments alone all reach the end
        // of the stream without a document.
        for input in ["", "  \n  ", "# just a comment\n"] {
            // Act
            let report = reject(input);

            // Assert
            assert_eq!(
                report.issues()[0].message,
                ROOT_MUST_BE_A_MAPPING,
                "input: {input:?}"
            );
            let span = report.issues()[0].span.unwrap();
            assert_eq!(span.start, 0);
        }
    }

    #[test]
    fn a_second_document_reports_rather_than_parsing_clean() {
        // Arrange
        // A loader with its multi flag off would parse the first document and
        // discard the rest, which is the loss this frontend refuses.
        let input = "a: 1\n---\nb: 2\n";

        // Act
        let report = reject(input);

        // Assert
        assert_eq!(report.issues()[0].message, ONE_DOCUMENT);
        let span = report.issues()[0].span.unwrap();
        assert_eq!(&input[span.start as usize..span.end as usize], "---");
    }

    #[test]
    fn an_alias_surfaces_as_a_type_mismatch_and_its_anchor_reads_through() {
        // Arrange
        let input = "base: \"anchored\"\nuse: *a\nlist:\n  - *a\n";
        let anchored = "base: &a \"anchored\"\nuse: *a\nlist:\n  - *a\n";

        // Act
        let fields = parse(anchored);

        // Assert
        let _ = input;
        let mut report = Report::new();
        // The anchored value is ordinary data wherever it stands.
        assert_eq!(
            parse_string_field(fields.get("base").unwrap(), &mut report)
                .unwrap()
                .value,
            "anchored"
        );
        assert!(parse_string_field(fields.get("use").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected string, found alias");
        let mut report = Report::new();
        assert!(parse_string_list_field(fields.get("list").unwrap(), &mut report).is_none());
        assert_eq!(report.issues()[0].message, "expected string, found alias");
    }

    #[test]
    fn an_alias_to_an_undefined_anchor_is_a_syntax_error() {
        // Arrange
        // The parser owns this judgment, so the undefined form is
        // all-or-nothing where the defined one is a field-level mismatch.
        let input = "use: *nope\n";

        // Act
        let report = reject(input);

        // Assert
        assert!(
            report.issues()[0].message.starts_with("syntax error: "),
            "got: {:?}",
            report.issues()
        );
    }

    #[test]
    fn a_non_scalar_key_reports_and_the_walk_continues() {
        // Arrange
        // One exotic key must not hide the fields after it.
        let input = "? [a, b]\n: 1\nport: 8080\n";
        let mut sources = SourceMap::new();
        let id = sources.add("test.yaml", input);
        let mut report = Report::new();

        // Act
        let fields = parse_yaml_fields(&sources, id, &mut report).unwrap();

        // Assert
        assert_eq!(report.issues()[0].message, SCALAR_KEY);
        assert!(fields.get("port").is_some(), "the later field must survive");
        assert_eq!(fields.iter().count(), 1);
    }

    #[test]
    fn a_merge_key_is_an_ordinary_key() {
        // Arrange
        // The merge convention is YAML 1.1, outside the 1.2 core schema, so a
        // spec that does not declare `<<` reports it as unknown.
        let input = "base: &b \"x\"\n<<: *b\nport: 1\n";

        // Act
        let fields = parse(input);

        // Assert
        let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["base", "<<", "port"]);
    }

    #[test]
    fn core_tags_resolve_and_the_non_specific_tag_reads_as_a_string() {
        // Arrange
        let input = "forced: !!str 8080\nnonspecific: ! 8080\ncounted: !!int 7\n";

        // Act
        let fields = parse(input);

        // Assert
        let mut report = Report::new();
        for name in ["forced", "nonspecific"] {
            assert_eq!(
                parse_string_field(fields.get(name).unwrap(), &mut report)
                    .unwrap()
                    .value,
                "8080",
                "field {name}"
            );
        }
        assert_eq!(
            parse_int_field(fields.get("counted").unwrap(), &mut report)
                .unwrap()
                .value,
            7
        );
        assert!(!report.has_issues());
    }

    #[test]
    fn every_refused_tag_reports_one_label() {
        // Arrange
        // Three producers reach `tagged value`: a core scalar tag whose text it
        // cannot read, a core tag on the wrong node kind, and a non-core tag.
        let input = "bad: !!int foo\nwrong: !!int {a: 1}\nmis: !!map \"text\"\ncustom: !custom 1\n";

        // Act
        let fields = parse(input);

        // Assert
        for name in ["bad", "wrong", "mis", "custom"] {
            let mut report = Report::new();
            assert!(parse_string_field(fields.get(name).unwrap(), &mut report).is_none());
            assert_eq!(
                report.issues()[0].message,
                "expected string, found tagged value",
                "field {name}"
            );
        }
        // The refused collection was consumed, so the fields after it survive.
        assert_eq!(fields.iter().count(), 4);
    }

    #[test]
    fn a_syntax_error_reports_one_issue_at_its_position() {
        // Arrange
        // A tab in block indentation is the parser's judgment, not the
        // frontend's.
        let input = "a:\n\tb: 1\n";

        // Act
        let report = reject(input);

        // Assert
        assert_eq!(report.issues().len(), 1);
        assert!(
            report.issues()[0].message.starts_with("syntax error: "),
            "got: {:?}",
            report.issues()
        );
        assert!(report.issues()[0].span.is_some());
    }
}
