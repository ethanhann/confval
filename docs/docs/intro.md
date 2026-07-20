---
sidebar_position: 1
---

# Overview

confval is a Rust crate for parsing, validating, and lowering configuration files.
It records a source span for every parsed value, so a validation error can report the line and column in the file the value came from.

Use it to build the configuration layer of an application.
You define the shape of the config as Rust types, parse a file into those types, run validation, and lower the result into the runtime types the rest of the program uses.

## Design

confval is built around five decisions.
Read them first, because the rest of the documentation assumes them.

- **Spans travel with values.**
  Every parsed value carries the byte range it came from.
  Any later check resolves that range to a line and column in the source file.
- **A run reports every problem it finds.**
  Parsing and validation append to a shared report instead of returning on the first error.
  The caller fixes one batch of problems rather than rerunning to find the next one.
- **Shape and meaning are checked in separate phases.**
  Parsing checks shape only: the field exists and has the right type.
  Range checks, closed sets, and cross-field rules run after parsing, in plain validation functions.
- **The core does not know any file format.**
  Parsing produces a format-neutral field model.
  A frontend converts one syntax into that model.
  HCL and TOML ship today, each behind its own feature, and a new format is another frontend over the same model.
- **The core has no dependencies.**
  serde, owo-colors, hcl-edit, toml_edit, and the derive macros are each behind a feature flag.

## Crate layout

confval is organized into four layers, each a module, plus a prelude.
The dependency direction is strictly downward: `format` builds on `pipeline`, which builds on `diagnostic`, which builds on `source`.

| Module                | Holds                                                                                       |
|-----------------------|---------------------------------------------------------------------------------------------|
| `confval::source`     | `Located`, `Span`, `SourceId`, `Source`, `SourceMap` (the "where")                          |
| `confval::diagnostic` | `Report`, `Issue`, `IssueBuilder`, `Severity`, the renderers (the "what")                   |
| `confval::pipeline`   | `Lower`, `LowerAuto`, `Validate`, `narrow`, `RangeConstraint`, `KeywordSet` (the transform) |
| `confval::format`     | the neutral field model (`field`) and the frontends (`hcl`, `toml`)                         |
| `confval::prelude`    | a glob re-export of the common imports across those layers                                  |

`use confval::prelude::*;` pulls the everyday names (`Located`, `Span`, `Report`, `Lower`, `Validate`, `narrow`, `RangeConstraint`, `KeywordSet`, and the derives) in one line.
The explicit module paths remain available when you want them.

## The pipeline contract

confval assumes a fixed ordering of stages, and the derives are designed around it:

1. **Parse** (structural): a frontend (`parse_hcl` or `parse_toml`) builds the neutral `Fields`, runs `FromFields`, and reports shape problems.
2. **Validate** (semantic): `Validate` impls take `&self` and `&mut Report` and check ranges, closed sets, and cross-field rules against the spans stored in `Located` fields.
   The trait doubles as a compile-time bound on step 4, so a lowerable spec without a validator does not compile.
3. **Gate**: lowering must not run on a report that contains errors.
4. **Lower**: `Lower::lower` converts specs to runtime types.
   Because the gate ran, narrowing conversions in `with` functions are safe.

For a runnable end-to-end example, see [Getting Started](./getting-started.md).
For the detail on each phase, read the guide.
