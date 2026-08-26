---
name: confval-add-block
description: Keep a confval pipeline in sync when a field or block is added, so the spec, validation, runtime type, lowering, and Default impl all stay aligned
---

# Add a field or block to a confval pipeline

You are adding a setting to a project that already has a confval pipeline.
A configuration setting runs through five layers: the spec, the validation, the runtime type, the lowering, and the `Default` impl.
A setting that reaches only some of them parses and lowers with no complaint.

Update all five layers for the setting you add.

The compiler catches some of the five for you.
The spec and the runtime type must agree.
The generated lowering destructures the spec exhaustively, so a field on one side and not the other is a compile error.
The validation is the silent one.
A field with no rule parses and lowers with no complaint.
A missing check is a no-op rather than an error.
Give the validation the most attention.

## The five places

Work through each one for the field or block you are adding.

### 1. The spec type

Add the field to the spec struct as a `Located<T>`.
Use `#[confval(nested)]` on a new block and give the block its own spec struct.
Use `Vec<Located<T>>` for a block that may repeat.
Hold a closed set of strings as a `Located<String>` with a `keyword_enum!` for its enum, rather than an enum in the spec.
Mark the child field that names a repeated block's instances with `#[confval(label)]`.
Mark a string field that points at one of those names with `#[confval(references = <block>)]`.

### 2. The validation

Declare a recorded constraint on the field.

- A numeric range with `#[confval(range = ...)]`.
- A character length bound with `#[confval(length = ...)]`.
- A parse as a named format with `#[confval(format = ...)]`.
- A closed set with `#[confval(keywords = ...)]`.
- A non-empty check with `#[confval(non_empty)]`.

The derive runs a recorded constraint during validation, so the `Validate` impl carries no line for it.
A recorded constraint expands where the spec struct is declared, so put the `range_constraint!` const, the `length_constraint!` const, and the keyword enum in that module.
A project that holds its constraints elsewhere will not compile until they move, and the compiler names the missing const rather than the reason.
Add a rule an attribute cannot express to the spec type's `Validate` impl.
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

When the runtime default comes from lowering the spec's `Default`, the spec attribute is the only place to update.
There is no second declaration to keep in agreement.

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
