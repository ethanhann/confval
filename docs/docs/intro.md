---
sidebar_position: 1
---

# Overview

confval is a standalone Rust crate for span-first configuration parsing, validation, and lowering.
It provides the generic primitives you use to turn a configuration file into a validated, executable runtime type, with every diagnostic pointing back at the exact line and column it came from.

## Design goals

- **Span-first.**
  Every parsed value carries the byte range it came from, so any later check can point at the exact line and column in the source file.
- **One-pass reporting.**
  Parsing and validation never stop at the first problem.
  Issues accumulate in a report and the operator sees everything at once.
- **Structural and semantic separation.**
  Parsing only checks shape (field exists, field has the right type).
  Semantic rules (ranges, closed sets, cross-field invariants) live in plain validation functions that run after parsing.
- **Format-neutral core.**
  Parsing produces a format-neutral field model.
  Only a thin frontend knows any concrete syntax.
  HCL and TOML ship today, each behind its own feature, and a new format is one more frontend over the same primitives.
- **Minimal dependencies.**
  The core has none.
  serde, owo-colors, hcl-edit, toml_edit, and the derive macros are all behind feature flags.

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

confval assumes a fixed phase ordering, and the derives are designed around it:

1. **Parse** (structural): a frontend (`parse_hcl` or `parse_toml`) builds the neutral `Fields`, runs `FromFields`, and reports shape problems.
2. **Validate** (semantic): `Validate` impls take `&self` and `&mut Report` and check ranges, closed sets, and cross-field rules against the spans stored in `Located` fields.
   The trait doubles as a compile-time bound on step 4, so a lowerable spec without a validator does not compile.
3. **Gate**: lowering must not run on a report that contains errors.
4. **Lower**: `Lower::lower` converts specs to runtime types.
   Because the gate ran, narrowing conversions in `with` functions are safe.

Start with [Getting Started](./getting-started.md) for a runnable end-to-end example, then read the guide for each layer.
