---
sidebar_position: 10
---

# The language server core

confval rejects an unknown field at process startup, so a mistake in a configuration written by hand surfaces as a hard failure rather than a silently ignored key.
A language server moves that feedback into the editor.
It makes the legal surface visible at the point of authorship, so an operator sees which fields are legal, what each one holds, and where the file is wrong before the program runs.

The `confval-lsp` crate is the schema-generic core of that server.
It is generic over your root spec and over the format frontend, so one core serves an HCL, a TOML, a KDL, a JSON, or a YAML document written against any confval schema.

## Running a server for your spec

Bind the core to your `#[derive(Spec)]` root and a frontend, then run it over stdio.
The `serve` function owns the connection, the initialize handshake, and the request loop.

For example, serve an HCL document written against a `ServerSpec`:

```rust
use confval_lsp::{serve, Hcl};

serve::<ServerSpec, Hcl>(Hcl)
```

The core needs only the traits the derive emits: `FromFields`, `Validate`, `ValidateNested`, and `ToSchema`.
Nothing in it is specific to one spec.
A subcommand that names your root spec and its frontend is the whole binding.

## Trying it against an editor

The crate ships a `serve` example that binds the core to a demo spec, so you can point an editor at a running server before writing your own.
Run it and choose a format:

```
cargo run -p confval-lsp --example serve hcl
```

The example serves over stdin and stdout, so an LSP client launches the built binary at `target/debug/examples/serve` and speaks to it.
The example uses a demo spec, so it is a way to see the feature set, not a deployment.
The real server names your own root spec.

## The feature set

The core answers the editor's schema-driven questions.

Diagnostics parse the whole buffer and run the real pipeline, `from_fields` then `validate_all`.
A diagnostic the editor shows is a diagnostic the program would produce, because the two run the same checks rather than an approximation.

Completion offers what is legal at the cursor.
A body position offers the attribute names and block types the schema declares there, minus the single-valued fields already set.
A value position for a field with a keyword set offers the allowed strings, which the schema carries from a `#[confval(keywords = ...)]` attribute.

Hover reads the field under the cursor.
It renders the field's doc comment, its declared type, whether it has a default, and its constraint.
It also states whether the field is set by the configuration or left to its default.
This state comes from the field's presence in the parsed file.

## The formats it serves

The core serves every format confval parses: HCL, TOML, KDL, JSON, and YAML.

The block-structured formats, HCL, TOML, and KDL, and JSON resolve a cursor through the parsed tree, and reconstruct it from the raw text while the buffer is mid-edit and does not parse.
JSON nests through object braces and array brackets, so a cursor inside an array element resolves into the element.

YAML nests by indentation rather than a delimiter, so the server reads the enclosing keys from the cursor's indentation.
It handles the common shapes, including a block sequence, a value on the next line, and an inline flow collection.
Because it reads structure from indentation, an unusual layout, such as a flow collection spread across several lines, can resolve less precisely than a block mapping.

## A note on encoding

An editor sends a position as a line and a character, and the character counts UTF-16 code units by default.
The server negotiates the position encoding at initialization and prefers UTF-8 when the client supports it, so a range over a non-ASCII value stays aligned.

## The standalone-buffer limitation

The server validates the open buffer as a standalone file.
When you assemble a configuration from several layers, a later layer can supply a value the open file omits.
A diagnostic that reports a missing required field is then a false positive, because the field is present once the layers combine.

Layer-aware validation needs the whole layer stack, which only the assembling program holds.
It is planned as a later addition.
