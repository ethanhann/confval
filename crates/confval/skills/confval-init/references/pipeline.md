# The pipeline, for the person writing it

You are building the four layers confval runs a configuration file through.
This file describes the four phases in order, the type each one produces, and where a source span comes from and where it is used.
It carries no format detail.
Which frontend parses the file is the subject of `references/frontends.md`.

The published contract for this sequence is in the complete confval documentation at https://ethanhann.com/confval/llms-full.txt.
That file tracks the latest release, so confirm any API against the confval version this project pins.
This file restates the contract for a reader who is writing the code rather than learning the library.

## The four phases

A configuration file moves through parse, validate, gate, and lower, in that order.

```text
configuration file
  │  parse     a frontend reads the text, structural and span-first
  ▼
spec          every leaf a Located<T> that holds its span
  │  validate  semantic rules, collected into a Report
  ▼
Report        errors and warnings, each at its span
  │  gate      lowering never runs while the report holds errors
  ▼
config        the resolved runtime types
```

Each phase may assume the phases before it have run.

### Parse

A frontend reads the file text and produces your spec type, wrapped in an `Option`.
The spec type is a plain struct whose every leaf field is a `Located<T>`, so each parsed value keeps the byte range it came from.
Parsing checks structure alone.
It reports a missing field, a wrong type, an unknown field, and a duplicate, each at its span.
It does not check what a value means.

An input whose tree was built keeps flowing into validation even when some of its fields failed to parse, so parse problems and validation problems arrive together.
Only a syntax error, which produces no tree, stops the load.

### Validate

Validation reads the spec and reports what the values mean against the spans the `Located` fields carry.
The rules for one spec type's own fields live in its `Validate` impl.
One call runs the whole tree.

```rust
spec.validate_all(&mut report);
```

`validate_all` runs a type's own `validate` and then descends into every `#[confval(nested)]` field, recursively, through the traversal `#[derive(Spec)]` generates.
A nested block added later is validated without anyone editing a validator.
Calling `validate` instead checks the root and stops there.

Validation never returns early on the first problem.
Every rule appends its issues to the same `Report`, so one run reports every violation rather than the first.
This is the property a round-trip test asserts.
It is also why a validator holds no `?` and no early `return`.

### Gate

Lowering must not run while the report holds errors.
Nothing in confval enforces this.
You call `report.has_errors()` after validation and stop before lowering when it is true.

```rust
if report.has_errors() {
    // render the report and exit, or reject the reload
    return;
}
```

`Report` also has `has_warnings()` and `has_issues()`.
You decide whether a warning stops the run.

### Lower

Lowering narrows the validated spec into the runtime types the rest of the program uses.
`#[derive(Config)]` writes it for a mechanical mapping, and a handwritten `Lower` impl covers a mapping that can fail in a way the derive cannot express.

```rust
let config = ServerConfig::lower(&spec, &mut report);
```

Because the gate ran, the narrowing conversions inside lowering are safe.
`lower` returns an `Option`, so handle the `None` and report it rather than unwrapping, because the runtime path never panics.
Lowering does not accumulate.
It reports one error and stops, because a lowering error means an earlier phase let something through rather than that the operator made a mistake.
Say so in that error's message.

When you write a spec by hand, import the traits that validation and lowering require.

```rust
use confval::prelude::{Lower, Span, Validate, ValidateNested};
```

## Where spans come from and where they go

A `Located<T>` pairs a value with a `Span`, a byte range in one source file.

```rust
pub struct Located<T> {
    pub value: T,
    pub span: Span,
}
```

The frontend sets the span on every leaf as it parses.
A value filled from a `#[confval(default)]` carries a detached span, because no source text stands behind it.
Validation consumes the span by passing it to `report.at(...)`, so a diagnostic points at the line and column.
A `narrow` helper does the same on the lowering side, so an out-of-range value that slipped past validation still reports at its source location.
Equality on `Located` ignores the span, so two configs with the same values compare equal whatever their formatting.

## More than one file

A configuration may span several files, and the four phases do not change.
One `SourceMap` holds every file, and one `Report` collects every issue.
A `Span` carries the id of the source it came from, so issues from different files merge into one report and render together, each at its own file and line.

Parse every file before you stop.
Record that a file failed to parse, and stop after the loop rather than at the first failure, so one run reports every syntax error rather than the first.
A file that produced a tree keeps flowing into validation even when some of its fields failed.
Only a file that produced no tree at all is absent from the later phases.

`Located::detached` supplies a value with no source position, for a configuration built in code rather than read from a file.
A validator must not assume a source entry exists, because a detached span has none.

## The two parallel structs

Each setting exists twice.
The spec struct is populated from the file with every field span-tracked.
The config struct is the resolved runtime form.

The spec holds the rawest type that parses without failing, which is `String`, `i64`, `f64`, `bool`, or `PathBuf`.
A closed set of strings is a `Located<String>` in the spec, checked against a keyword set in validation, and converted to an enum at lowering.
The config holds the fully typed form, so downstream code never re-parses a string it read from configuration.

The generated lowering destructures the spec exhaustively, so adding a field to one side without accounting for it on the other is a compile error.

## An end to end program

This program runs all four phases.
Read it once for the shape, then build your own layers around your domain model.

```rust
use confval::prelude::*;

range_constraint!(PORT, i64, min: 1, max: 65535);
length_constraint!(HOSTNAME_LEN, max: 253);

#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(non_empty, length = HOSTNAME_LEN)]
    hostname: Located<String>,
    #[confval(range = PORT)]
    port: Located<i64>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    hostname: String,
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
}

fn main() {
    let text = "hostname = \"127.0.0.1\"\nport = 8080\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", text);

    // A syntax error yields None, with the reason already in the report. Never
    // panic on a misconfiguration.
    let Some(spec): Option<ServerSpec> =
        confval::format::toml::parse_toml(&sources, id, &mut report)
    else {
        return;
    };

    spec.validate_all(&mut report);
    if report.has_errors() {
        // render the report and stop before lowering
        return;
    }

    // A lowering error means a validation rule is missing, so report it rather
    // than unwrapping.
    if let Some(config) = ServerConfig::lower(&spec, &mut report) {
        println!("{}:{}", config.hostname, config.port);
    }
}
```
