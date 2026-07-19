---
sidebar_position: 2
---

# The diagnostic module

`confval::diagnostic` holds the "what": the report that collects issues and the renderers that turn them into operator-facing output.

## Report and IssueBuilder

`Report` collects issues.
Validators receive `&mut Report` and emit through a builder:

```rust
report
    .error("port must be between 1 and 65535")
    .at(spec.port.span)
    .help("Choose a port in the range 1..=65535.")
    .emit();
```

- `report.error(msg)` and `report.warning(msg)` return an `IssueBuilder`.
- `.at(span)` attaches the primary span.
  Issues without a span render without a source location.
- `.help(text)` adds a suggestion line.
- `.related(span, label)` attaches secondary spans, used for messages like "first declared here".
- `.emit()` finalizes the issue.
  The builder is `#[must_use]`, so forgetting `.emit()` is a compile-time warning.

Query methods: `has_errors()`, `has_warnings()`, `has_issues()`.
Severity is the two-variant `Severity` enum (`Error`, `Warning`).

## Rendering

Renderers write into any `fmt::Write` sink and take the `SourceMap` to resolve spans:

| Method          | Feature gate     | Format                                          |
|-----------------|------------------|-------------------------------------------------|
| `render_plain`  | always available | One line per issue with `file:line:col`, for CI |
| `render_pretty` | `color`          | rustc-style caret output with source excerpts   |
| `render_json`   | `serde`          | Structured JSON for tooling                     |

```rust
let mut out = String::new();
report.render_pretty(&sources, &mut out)?;
eprint!("{out}");
```

Pretty output underlines the offending value in its source line:

```
error: unknown load_balancing_strategy: failovr
  --> ingress.d/api.hcl:12:31
   |
12 |   load_balancing_strategy = "failovr"
   |                             ^^^^^^^^^
   = help: expected one of: failover, round_robin, request_pressure, sticky_hash, random
```

Line and column lookups are O(log n) via a per-source line index.
Columns count characters, not bytes.
An issue records only a severity, message, optional span, optional help, and related spans, and never touches source text until render time.
