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
    /// header, a container's range extends to the next sibling or the level
    /// end, the same extension cursor resolution applies, so a TOML
    /// container's range contains its children.
    pub covers_body: bool,
    /// Whether the client renders the hierarchical tree. Without it the flat
    /// form answers.
    pub hierarchical: bool,
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
    let symbols = level_symbols(schema, fields, shape.covers_body, text.len());
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
fn level_symbols(
    schema: &Schema,
    fields: &Fields,
    covers_body: bool,
    text_len: usize,
) -> Vec<RawSymbol> {
    let level: Vec<&Field> = fields.iter().collect();
    let enclosing_end = span_end(fields.enclosing()).unwrap_or(text_len as u32);
    let mut symbols = Vec::new();
    for (position, field) in level.iter().enumerate() {
        let Some(declared) = schema.fields.iter().find(|f| f.name == field.name) else {
            continue;
        };
        let next_start = level
            .get(position + 1)
            .and_then(|sibling| span_start(sibling.span));
        symbols.extend(field_symbols(
            field,
            &declared.ty,
            covers_body,
            next_start,
            enclosing_end,
            text_len,
        ));
    }
    symbols
}

/// The symbols one field contributes: one leaf symbol, or one container per
/// block instance.
fn field_symbols(
    field: &Field,
    declared: &SchemaType,
    covers_body: bool,
    next_start: Option<u32>,
    enclosing_end: u32,
    text_len: usize,
) -> Vec<RawSymbol> {
    match declared {
        SchemaType::Block { schema: inner, .. } => instances(field)
            .into_iter()
            .map(|(body, instance_span)| {
                container(
                    field,
                    inner,
                    body,
                    instance_span,
                    covers_body,
                    next_start,
                    enclosing_end,
                    text_len,
                )
            })
            .collect(),
        SchemaType::StringList => leaf(field, SymbolKind::ARRAY, text_len)
            .into_iter()
            .collect(),
        SchemaType::StringMap => leaf(field, SymbolKind::OBJECT, text_len)
            .into_iter()
            .collect(),
        _ => leaf(field, SymbolKind::FIELD, text_len)
            .into_iter()
            .collect(),
    }
}

/// One container symbol for one block instance, with its label as the detail
/// and its children walked recursively.
#[allow(clippy::too_many_arguments)]
fn container(
    field: &Field,
    inner_schema: &Schema,
    body: &Fields,
    instance_span: Span,
    covers_body: bool,
    next_start: Option<u32>,
    enclosing_end: u32,
    text_len: usize,
) -> RawSymbol {
    let start = span_start(instance_span)
        .or_else(|| span_start(field.name_span))
        .unwrap_or(0) as usize;
    // A header-only block's span stops at its header, so the range extends to
    // the next sibling or the level end, keeping the children contained.
    let end = if covers_body {
        span_end(instance_span).unwrap_or(0).max(deepest_end(body))
    } else {
        next_start.unwrap_or(enclosing_end).max(deepest_end(body))
    } as usize;
    let end = end.min(text_len).max(start);
    let selection = clamp(field.name_span, (start, end), text_len);
    let detail = instance_label(body, inner_schema);
    RawSymbol {
        name: field.name.clone(),
        detail,
        kind: SymbolKind::STRUCT,
        range: (start, end),
        selection,
        children: level_symbols(inner_schema, body, covers_body, text_len),
    }
}

/// One leaf symbol over the field's own span.
fn leaf(field: &Field, kind: SymbolKind, text_len: usize) -> Option<RawSymbol> {
    let start = span_start(field.span)? as usize;
    let end = (span_end(field.span)? as usize).min(text_len).max(start);
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

/// The furthest non-detached end offset among a level's fields and their
/// descendants, so an extended container still covers a nested table that runs
/// past its parent's header.
fn deepest_end(fields: &Fields) -> u32 {
    let mut furthest = 0;
    for field in fields.iter() {
        furthest = furthest.max(span_end(field.span).unwrap_or(0));
        if let FieldKind::Block(inner) = &field.kind {
            furthest = furthest.max(deepest_end(inner));
        }
        if let FieldKind::Value(value) = &field.kind {
            furthest = furthest.max(deepest_value_end(value));
        }
    }
    furthest
}

/// The furthest end within a value, recursing through maps and sequences.
fn deepest_value_end(value: &confval::format::Value) -> u32 {
    let mut furthest = span_end(value.span).unwrap_or(0);
    match &value.kind {
        ValueKind::Map(inner) => furthest = furthest.max(deepest_end(inner)),
        ValueKind::Seq(items) => {
            for item in items {
                furthest = furthest.max(deepest_value_end(item));
            }
        }
        _ => {}
    }
    furthest
}

/// The selection range: the name span clamped inside the symbol's range, or
/// the range start when the name has no span.
fn clamp(name_span: Span, range: (usize, usize), text_len: usize) -> (usize, usize) {
    match (span_start(name_span), span_end(name_span)) {
        (Some(start), Some(end)) => {
            let start = (start as usize).clamp(range.0, range.1).min(text_len);
            let end = (end as usize).clamp(start, range.1).min(text_len);
            (start, end)
        }
        _ => (range.0, range.0),
    }
}

fn span_start(span: Span) -> Option<u32> {
    (!span.is_detached()).then_some(span.start)
}

fn span_end(span: Span) -> Option<u32> {
    (!span.is_detached()).then_some(span.end)
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
