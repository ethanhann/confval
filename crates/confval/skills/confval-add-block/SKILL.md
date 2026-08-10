---
name: confval-add-block
description: Keep a confval pipeline in sync when a field or block is added, so the spec, validation, runtime type, lowering, and Default impl all stay aligned
---

# Add a field or block to a confval pipeline

You are adding a setting to a project that already has a confval pipeline.
A configuration setting is not one edit.
It runs through the spec, the validation, the runtime type, the lowering, and the `Default` impl, and a setting that reaches only some of those layers is a silent gap.

Your job is that none of the five is missed.

## The five places

Work through each one for the field or block you are adding.

### 1. The spec type

Add the field to the spec struct as a `Located<T>`.
Use `#[confval(nested)]` on a new block and give the block its own spec struct.
Use `Vec<Located<T>>` for a block that may repeat.
Hold a closed set of strings as a `Located<String>` with a `keyword_enum!` for its enum, rather than an enum in the spec.

### 2. The validation

Add the field's rule to the spec type's `Validate` impl.
A new block needs its own `Validate` impl, which `validate_all` reaches on its own through the generated traversal, so you do not call it by hand.
Report at the field's span and accumulate into the `Report` with no early return.

### 3. The runtime type

Add the field to the config struct in its resolved form, with no `Located` wrapper.
The generated lowering destructures the spec exhaustively, so a field added to the spec and not to the config is a compile error rather than a silent drop.
That compile error is your reminder, not a problem to work around.

### 4. The lowering

Map the new field in the config's `#[confval(lower(...))]` attribute, or in the handwritten `Lower` impl.
Narrow an integer with a `narrow` helper, and convert a keyword string with `narrow::keyword::<T>`.

### 5. The Default impl

When the spec type carries `#[confval(derive_default)]`, give the new field a `#[confval(default = ...)]` so the derived `Default` still builds.
When the type has a handwritten `impl Default`, add the field there too.
A field with no default on a `derive_default` type is a compile error.

## What not to collapse

Validation and lowering both check some of the same conditions.
Validation rejects an out-of-range value at its span and reports it alongside every other problem.
A `narrow` helper in lowering checks the same bound again and reports the value a validation rule missed.

That duplication is deliberate.
Validation accumulates every problem for the operator, and lowering is the safety net that catches a rule you forgot to write.
Collapsing the two into one phase changes behavior rather than cleaning anything up, because it removes the accumulating pass or the net.
Leave both in place.

## Verify

Run `cargo check`, then `cargo test`.
Read the new field back across all five layers against the current confval crate before you finish.
For the confval API details, read the complete documentation at https://ethanhann.com/confval/llms-full.txt, which tracks the latest release.

## Provenance

This skill was written for confval {{confval_version}}.
