---
sidebar_position: 5
---

# Contributing

## Just recipes

Various just recipes are available, but these are the most useful:

- `just docs`: run the docs site locally
- `just validate`: Test everything (lint, unit tests, etc.)

## Design

confval is built around five design decisions.
These design decisions should be adhered to.

### Spans travel with values.

- Every parsed value carries the byte range it came from.
- Any later check resolves that range to a line and column in the source file.

### All errors are collected and displayed to an operator.

- Parsing and validation append to a shared report instead of returning on the first error.
- The caller fixes one batch of problems rather than rerunning to find the next one.

### Validation happens in stages.

- Parsing checks shape only, meaning the field exists and has the right type.
- Range checks, closed sets, and cross-field rules run after parsing, in plain validation functions.

### The core does not know any file format.

- Parsing produces a format-neutral field model.
- A frontend converts one syntax into that model.
- HCL and TOML ship today, each behind its own feature, and a new format is another frontend over the same model.

### The core has no required dependencies.

serde, owo-colors, hcl-edit, toml_edit, and the derive macros are each behind a feature flag.
 
confval aims to stay free of required dependencies.
Put any new dependency behind a feature flag.

## Crate layout

confval is organized into four layers, each a module, plus a prelude.
The dependency direction is strictly downward: `format` builds on `pipeline`, which builds on `diagnostic`, which builds
on `source`.

| Module                | Holds                                                                                       |
|-----------------------|---------------------------------------------------------------------------------------------|
| `confval::source`     | `Located`, `Span`, `SourceId`, `Source`, `SourceMap` (the "where")                          |
| `confval::diagnostic` | `Report`, `Issue`, `IssueBuilder`, `Severity`, the renderers (the "what")                   |
| `confval::pipeline`   | `Lower`, `LowerAuto`, `Validate`, `narrow`, `RangeConstraint`, `KeywordSet` (the transform) |
| `confval::format`     | the neutral field model (`field`) and the frontends (`hcl`, `toml`)                         |
| `confval::prelude`    | a glob re-export of the common imports across those layers                                  |

`use confval::prelude::*;` pulls the everyday names (`Located`, `Span`, `Report`, `Lower`, `Validate`, `narrow`,
`RangeConstraint`, `KeywordSet`, and the derives) in one line.
