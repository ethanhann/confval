---
sidebar_position: 1
---

# The source module

`confval::source` holds the "where" of every value: the primitives that record and resolve source locations.

## Span and SourceId

A `Span` is a byte range inside one registered source:

```rust
pub struct Span {
    pub source: SourceId,
    pub start: u32,
    pub end: u32,
}
```

`SourceId` is a lightweight handle issued by the `SourceMap`.
Spans are plain data.
Resolving them to line and column numbers happens only at render time.

## SourceMap

The `SourceMap` interns source text.
Each file (or in-memory string) is registered once and identified by its `SourceId`:

```rust
let mut sources = SourceMap::new();
let id = sources.add("config.hcl", text);
```

Reports do not own source text.
Renderers take `&SourceMap` so the text is stored exactly once no matter how many issues reference it.

## Located

`Located<T>` pairs a value with the span it was parsed from:

```rust
pub struct Located<T> {
    pub value: T,
    pub span: Span,
}
```

Key behaviors:

- **Value-only equality.**
  `PartialEq`, `Eq`, and `Hash` ignore the span, so two configs with the same values compare equal regardless of formatting.
- **`Deref` to `T`.**
  Method calls pass through to the inner value.
- **`Located::detached(value)`** produces a value with a sentinel span.
  This is how specs are constructed in code (tests, builders, generated templates) without a source file.
- **`Default`** is `detached(T::default())`.
- With the `serde` feature, `Located<T>` serializes transparently as `T` and deserializes detached.
