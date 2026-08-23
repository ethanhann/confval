---
sidebar_position: 6
---

# Contributing

## Just recipes

Various just recipes are available, but these are the most useful:

- `just docs`: run the docs site locally
- `just validate`: test everything, including lint and unit tests
- `just mutants`: run mutation testing to find gaps the suite does not cover

`just validate` is the gate a change has to pass.
`just mutants` runs longer and is worth running when you add a module or change how one behaves.
It builds the whole workspace once per mutant, so expect it to take a while.
`confval-derive` has no tests of its own, which is why `.cargo/mutants.toml` sets `test_workspace`.
Its behavior is covered by the integration and trybuild tests in the `confval` package.

## Design

confval follows five design decisions.
A change that departs from one of these decisions needs a reason in the pull request.

### Spans travel with values

- Every parsed value carries the byte range it came from.
- Any later check resolves that range to a line and column in the source file.

### All errors are collected and displayed to an operator

- Parsing and validation append to a shared report instead of returning on the first error.
- The caller fixes one batch of problems rather than rerunning to find the next one.

### Validation happens in stages

- Parsing checks shape only, meaning the field exists and has the right type.
- Range checks, closed sets, and cross-field rules run after parsing, in plain validation functions.

### The core does not know any file format

- Parsing produces a format-neutral field model.
- A frontend converts one syntax into that model.
- HCL, TOML, KDL, JSON, and YAML ship today, each behind its own feature.
  A new format is another frontend over the same model.

### The core has no required dependencies

serde, annotate-snippets, hcl-edit, toml_edit, kdl, jsonc-parser, saphyr-parser, and the derive macros are each behind a feature flag.

confval aims to stay free of required dependencies.
Put any new dependency behind a feature flag.

## Examples

The [Examples](./examples.md) page lists each example's run command.
If you add an example or change one's required features, update that page and the `examples` recipe to match.
`just examples` prints every example's output for review.

## Crate layout

confval is organized into six modules, plus a prelude.
The dependency direction is strictly downward.
`pipeline` builds on `format`, which builds on `diagnostic`, which builds on `source`.
`layering` builds on `format`.
`schema` depends on no other module.

| Module                | Holds                                                                                       |
|-----------------------|---------------------------------------------------------------------------------------------|
| `confval::source`     | `Located`, `Span`, `SourceId`, `Source`, `SourceMap` (the "where")                          |
| `confval::diagnostic` | `Report`, `Issue`, `IssueBuilder`, `Severity`, the renderers (the "what")                   |
| `confval::pipeline`   | `Lower`, `LowerAuto`, `Validate`, `narrow`, `RangeConstraint`, `KeywordSet` (the transform) |
| `confval::format`     | the neutral field model (`field`) and the frontends (`hcl`, `json`, `kdl`, `toml`, `yaml`)          |
| `confval::layering`   | `Assembly` and the `env_fields` and `cli_fields` providers (the merge)                      |
| `confval::schema`     | `Schema`, `SchemaField`, `SchemaType`, `Constraint`, `ToSchema` (the type-level view)       |
| `confval::prelude`    | a glob re-export of the common imports across those layers                                  |

`use confval::prelude::*;` pulls the everyday names (`Located`, `Span`, `Report`, `Lower`, `Validate`, `narrow`, `RangeConstraint`, `KeywordSet`, and the derives) in one line.
