//! The furthest-end walks over a parsed subtree. Each walk answers how far
//! a block's content reaches, with and without the block spans themselves.

use confval::format::{FieldKind, Fields, Value, ValueKind};
use confval::source::Span;

use super::end_of;

/// How far an extent walk reaches.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Extent {
    /// Counts block and map spans beside scalar ends. The document-symbol
    /// outline and cursor resolution share this walk, so a container's
    /// extension cannot differ between them.
    Deepest,
    /// Counts scalar ends only. A header or indentation format's block span
    /// runs to the next sibling, so a fold ends at the last entry instead.
    /// An empty block or map still counts its own span, because it has no
    /// entry to end at.
    Entries,
}

/// A block's body extent: the furthest end among the block's own span and its
/// descendants. A TOML `[table]` span covers only the header, so the block's
/// entries extend the body past it. HCL and KDL block spans already cover their
/// entries, so the furthest end leaves them unchanged.
pub(super) fn block_body_end(block_span: Span, inner: &Fields) -> u32 {
    end_of(block_span).max(furthest_end(inner, Extent::Deepest))
}

/// The furthest non-detached end offset among a level's fields and their
/// descendants, under the given [`Extent`] rule.
pub(crate) fn furthest_end(fields: &Fields, extent: Extent) -> u32 {
    let include_block_spans = extent == Extent::Deepest;
    let mut furthest = 0;
    for field in fields.iter() {
        let end = match &field.kind {
            FieldKind::Block(inner) => {
                let own = if include_block_spans || inner.iter().next().is_none() {
                    end_of(field.span)
                } else {
                    end_of(field.name_span)
                };
                own.max(furthest_end(inner, extent))
            }
            FieldKind::Value(value) => {
                let own = if include_block_spans {
                    end_of(field.span)
                } else {
                    0
                };
                own.max(furthest_end_value(value, extent))
            }
        };
        furthest = furthest.max(end);
    }
    furthest
}

/// The furthest non-detached end offset within a value, recursing through maps
/// and sequences under the same rule as [`furthest_end`].
pub(crate) fn furthest_end_value(value: &Value, extent: Extent) -> u32 {
    let include_block_spans = extent == Extent::Deepest;
    match &value.kind {
        ValueKind::Map(inner) => {
            if include_block_spans || inner.iter().next().is_none() {
                end_of(value.span).max(furthest_end(inner, extent))
            } else {
                furthest_end(inner, extent)
            }
        }
        ValueKind::Seq(items) => {
            let own = if include_block_spans || items.is_empty() {
                end_of(value.span)
            } else {
                0
            };
            items
                .iter()
                .map(|item| furthest_end_value(item, extent))
                .fold(own, u32::max)
        }
        _ => end_of(value.span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confval::format::{Field, Scalar};
    use confval::source::SourceMap;

    #[test]
    fn deepest_end_value_reaches_past_a_maps_own_span_to_its_deepest_child() {
        // Arrange
        // The map value's own span ends at 10, but a child inside it ends at 50.
        // The furthest end must follow the map into its fields, not stop at the
        // map's own end.
        let mut sources = SourceMap::new();
        let id = sources.add("map", "x");
        let child = Field::parsed(
            "deep",
            Span::new(id, 40, 44),
            Span::new(id, 40, 50),
            id,
            FieldKind::Value(Value::spanned(
                Span::new(id, 45, 50),
                ValueKind::Scalar(Scalar::Int(1)),
            )),
        );
        let inner = Fields::new(id, Span::new(id, 30, 55), vec![child]);
        let map_value = Value::spanned(Span::new(id, 5, 10), ValueKind::Map(inner));

        // Act
        let furthest = furthest_end_value(&map_value, Extent::Deepest);

        // Assert
        assert_eq!(furthest, 50);
    }
}
