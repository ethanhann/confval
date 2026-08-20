//! The document-symbol handler: the outline of the parsed document.
//!
//! The tree pairs each parsed field with its schema entry, so a block instance
//! is a container symbol carrying its label as the detail, and a leaf field is
//! a leaf symbol. The hierarchical form answers a client that declares support
//! for it, and the flat form answers the rest. A buffer that does not parse
//! answers empty, because the outline reads spans only a parse provides.

use lsp_types::{
    DocumentSymbol, DocumentSymbolResponse, Location, SymbolInformation, SymbolKind, Uri,
};

use confval::format::{Field, FieldKind, Fields, ValueKind};
use confval::schema::{Schema, SchemaType};
use confval::source::Span;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::resolve::deepest_end;

/// One symbol with byte ranges, before position encoding.
struct RawSymbol {
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: (usize, usize),
    selection: (usize, usize),
    children: Vec<RawSymbol>,
}

/// The two answers that shape a symbol response: the frontend's block-span
/// form and the client's hierarchy support.
pub struct SymbolShape {
    /// The frontend's block-span answer. When a block's span covers only its
    /// header, the container's range extends to the next sibling or the level
    /// end. Cursor resolution applies the same extension. A TOML container's
    /// range therefore contains its children.
    pub covers_body: bool,
    /// Whether the client renders the hierarchical tree. Without it the flat
    /// form answers.
    pub hierarchical: bool,
}

/// The build inputs every level shares: the frontend's block-span answer and
/// the text bound the ranges clamp to.
struct Build {
    covers_body: bool,
    text_len: usize,
}

/// Produces the document symbols for a parsed document.
pub fn document_symbols(
    schema: &Schema,
    fields: &Fields,
    shape: SymbolShape,
    uri: &Uri,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> DocumentSymbolResponse {
    let build = Build {
        covers_body: shape.covers_body,
        text_len: text.len(),
    };
    let symbols = level_symbols(schema, fields, &build);
    if shape.hierarchical {
        DocumentSymbolResponse::Nested(
            symbols
                .into_iter()
                .map(|symbol| encode(symbol, text, index, encoding))
                .collect(),
        )
    } else {
        let mut flat = Vec::new();
        flatten(symbols, None, uri, text, index, encoding, &mut flat);
        DocumentSymbolResponse::Flat(flat)
    }
}

/// The symbols of one level, in document order, skipping fields the schema
/// does not declare.
fn level_symbols(schema: &Schema, fields: &Fields, build: &Build) -> Vec<RawSymbol> {
    let level: Vec<&Field> = fields.iter().collect();
    let enclosing_end =
        span_range(fields.enclosing()).map_or(build.text_len as u32, |range| range.1);
    let mut symbols = Vec::new();
    for (position, field) in level.iter().enumerate() {
        let Some(declared) = schema.fields.iter().find(|f| f.name == field.name) else {
            continue;
        };
        let next_start = level
            .get(position + 1)
            .and_then(|sibling| span_range(sibling.span).map(|range| range.0));
        symbols.extend(field_symbols(
            field,
            &declared.ty,
            next_start,
            enclosing_end,
            build,
        ));
    }
    symbols
}

/// The symbols one field contributes: one leaf symbol, or one container per
/// block instance.
fn field_symbols(
    field: &Field,
    declared: &SchemaType,
    next_start: Option<u32>,
    enclosing_end: u32,
    build: &Build,
) -> Vec<RawSymbol> {
    match declared {
        SchemaType::Block { schema: inner, .. } => {
            let instances = instances(field);
            let count = instances.len();
            instances
                .iter()
                .enumerate()
                .map(|(position, (body, instance_span))| {
                    // Each instance ends at its following sibling instance, and
                    // only the last extends to the field-level bound, so
                    // repeated header-only tables keep disjoint ranges.
                    let bound = if position + 1 < count {
                        instances
                            .get(position + 1)
                            .and_then(|(_, next_span)| span_range(*next_span))
                            .map(|range| range.0)
                            .or(next_start)
                    } else {
                        next_start
                    };
                    container(
                        field,
                        inner,
                        body,
                        *instance_span,
                        bound,
                        enclosing_end,
                        build,
                    )
                })
                .collect()
        }
        SchemaType::StringList { .. } => leaf(field, SymbolKind::ARRAY, build.text_len)
            .into_iter()
            .collect(),
        SchemaType::StringMap => leaf(field, SymbolKind::OBJECT, build.text_len)
            .into_iter()
            .collect(),
        _ => leaf(field, SymbolKind::FIELD, build.text_len)
            .into_iter()
            .collect(),
    }
}

/// One container symbol for one block instance, with its label as the detail
/// and its children walked recursively.
fn container(
    field: &Field,
    inner_schema: &Schema,
    body: &Fields,
    instance_span: Span,
    bound: Option<u32>,
    enclosing_end: u32,
    build: &Build,
) -> RawSymbol {
    let start = span_range(instance_span)
        .or_else(|| span_range(field.name_span))
        .map_or(0, |range| range.0) as usize;
    // A header-only block's span stops at its header, so the range extends to
    // the next sibling or the level end, keeping the children contained.
    let end = if build.covers_body {
        span_range(instance_span)
            .map_or(0, |range| range.1)
            .max(deepest_end(body))
    } else {
        bound.unwrap_or(enclosing_end).max(deepest_end(body))
    } as usize;
    let end = end.min(build.text_len).max(start);
    let selection = clamp(field.name_span, (start, end), build.text_len);
    let detail = instance_label(body, inner_schema);
    RawSymbol {
        name: field.name.clone(),
        detail,
        kind: SymbolKind::STRUCT,
        range: (start, end),
        selection,
        children: level_symbols(inner_schema, body, build),
    }
}

/// One leaf symbol over the field's own span.
fn leaf(field: &Field, kind: SymbolKind, text_len: usize) -> Option<RawSymbol> {
    let (span_start, span_end) = span_range(field.span)?;
    let start = span_start as usize;
    let end = (span_end as usize).min(text_len).max(start);
    Some(RawSymbol {
        name: field.name.clone(),
        detail: None,
        kind,
        range: (start, end),
        selection: clamp(field.name_span, (start, end), text_len),
        children: Vec::new(),
    })
}

/// A block instance's label for the detail: the native slot, or the value of
/// the designated label field.
fn instance_label(body: &Fields, schema: &Schema) -> Option<String> {
    if let Some(label) = body.label() {
        return Some(label.value.clone());
    }
    let label_field = schema.fields.iter().find(|field| field.label)?;
    match &body.get(&label_field.name)?.kind {
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Scalar(confval::format::Scalar::String(text)) => Some(text.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The bodies of one block field, one per instance, each with its span.
fn instances(field: &Field) -> Vec<(&Fields, Span)> {
    match &field.kind {
        FieldKind::Block(body) => vec![(body, field.span)],
        FieldKind::Value(value) => match &value.kind {
            ValueKind::Map(body) => vec![(body, field.span)],
            ValueKind::Seq(elements) => elements
                .iter()
                .filter_map(|element| match &element.kind {
                    ValueKind::Map(body) => Some((body, element.span)),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        },
    }
}

/// The selection range: the name span clamped inside the symbol's range, or
/// the range start when the name has no span.
fn clamp(name_span: Span, range: (usize, usize), text_len: usize) -> (usize, usize) {
    match span_range(name_span) {
        // A name span outside the symbol's range, such as the field-level
        // header of a later instance, yields a zero-width selection at the
        // instance start rather than a span clamped across the boundary.
        Some((start, end)) if (start as usize) >= range.0 && (end as usize) <= range.1 => {
            ((start as usize).min(text_len), (end as usize).min(text_len))
        }
        _ => (range.0, range.0),
    }
}

/// A span's byte range, or `None` for the detached sentinel.
fn span_range(span: Span) -> Option<(u32, u32)> {
    (!span.is_detached()).then_some((span.start, span.end))
}

/// Encodes one raw symbol into the protocol shape.
fn encode(
    symbol: RawSymbol,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> DocumentSymbol {
    #[allow(deprecated)]
    DocumentSymbol {
        name: symbol.name,
        detail: symbol.detail,
        kind: symbol.kind,
        tags: None,
        deprecated: None,
        range: index.range_of_bytes(text, symbol.range, encoding),
        selection_range: index.range_of_bytes(text, symbol.selection, encoding),
        children: Some(
            symbol
                .children
                .into_iter()
                .map(|child| encode(child, text, index, encoding))
                .collect(),
        ),
    }
}

/// Flattens the tree into the non-hierarchical form, with each symbol carrying
/// its container's name.
fn flatten(
    symbols: Vec<RawSymbol>,
    container: Option<&str>,
    uri: &Uri,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    out: &mut Vec<SymbolInformation>,
) {
    for symbol in symbols {
        #[allow(deprecated)]
        out.push(SymbolInformation {
            name: symbol.name.clone(),
            kind: symbol.kind,
            tags: None,
            deprecated: None,
            location: Location {
                uri: uri.clone(),
                range: index.range_of_bytes(text, symbol.range, encoding),
            },
            container_name: container.map(str::to_string),
        });
        flatten(
            symbol.children,
            Some(&symbol.name),
            uri,
            text,
            index,
            encoding,
            out,
        );
    }
}
