---
name: confval-init
description: Scaffold a confval configuration pipeline in a Rust project that has none, building the spec, validation, and runtime layers around the project's own configuration format
---

# Set up a confval pipeline

You are adding confval to a Rust project that parses a configuration file, or that should.
confval turns a configuration file into validated runtime types.
It keeps a source span on every value, so a diagnostic points at the line and column the value came from.

Your job is to build three layers around the project's domain model.
The spec layer parses the file into span-tracked types.
Validation checks what the values mean and reports every problem in one pass.
The runtime layer is the resolved form the rest of the program uses.

This skill scaffolds those layers.
It does not write the project's domain rules, it does not decide the gate stage, and it does not generate schema files.
Those belong to the project.

Read the pipeline before you start.
The [pipeline reference](references/pipeline.md) covers the four phases and where a span comes from.
The [frontends reference](references/frontends.md) covers the format features and what each format can express.
The [patterns reference](references/patterns.md) covers `keyword_enum!`, `#[confval(derive_default)]`, the `narrow` helpers, and template mode.

For anything the references do not cover, read the complete confval documentation at https://ethanhann.com/confval/llms-full.txt.
That file tracks the latest release, so confirm any API against the confval version this project pins.

## Steps

### 1. Survey the project

Find where configuration already lives.
Look for an existing configuration struct, a hand-rolled parser, or a sample configuration file.
Read the format from the file extension: `.toml`, `.hcl`, `.kdl`, `.json`, or `.yaml`.
When no file settles the format, ask which one to target.
Offer all five formats: HCL, TOML, KDL, JSON, and YAML.

A project that already has a configuration system forces two more choices.
First, decide which configuration surface to target when the project has more than one.
Second, decide whether the confval layer runs beside the existing loader or replaces it.
Ask the operator to settle each choice before you write code.

Ask also whether the configuration is one file or several.
A configuration that spans several files needs a way to find the others, such as a list or a glob in an entry file.
That discovery is the project's decision rather than something confval supplies.

### 2. Add the dependency

confval has no default features, so you enable the ones the project uses.

```shell
cargo add confval@{{confval_version}} --features derive,toml
```

Enable `derive` always.
Enable the one frontend feature that matches the format, `hcl`, `toml`, `kdl`, `json`, or `yaml`.
Add `color` when the project renders diagnostics to a terminal.
Add `serde` when the project needs JSON diagnostics or a serializable config.
Add `layering` only when more than one source is combined into one configuration.
Name no feature the project does not use.

### 3. Write the spec layer

The spec is a plain struct whose every leaf field is a `Located<T>`, so each value keeps its span.
Derive the parser with `#[derive(confval::Spec)]`.

```rust
#[derive(confval::Spec)]
struct ServerSpec {
    hostname: Located<String>,
    port: Located<i64>,
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
}
```

Use one nested struct per block, marked `#[confval(nested)]`.
Use `Vec<Located<T>>` for a block that may repeat.
Hold a closed set of strings as a `Located<String>` and declare its enum with `keyword_enum!`.
Add `#[confval(derive_default)]` where a block needs a `Default` built from its attribute defaults.
Store the rawest type that parses without failing, which is `String`, `i64`, `f64`, `bool`, or `PathBuf`.
Narrow it later, at lowering.

The derive handles plain structs.
A shape it cannot express needs a handwritten `FromFields` impl.
The cases are:

- a tagged block that dispatches on a discriminator field
- a free-form block held as an arbitrary value

A string-keyed map has a derive form, `#[confval(map)]` over a `BTreeMap<String, Located<String>>`, so it needs no handwritten impl.

See the complete documentation for the handwritten path.

### 4. Write validation

Declare a mechanical constraint on the field that carries it.
Each recording attribute names its declaration.

- `#[confval(range = PORT)]` records a `range_constraint!`.
- `#[confval(length = HOSTNAME_LEN)]` records a `length_constraint!`.
- `#[confval(format = Ip)]` records a `Format` type.
- `#[confval(keywords = LimitMode)]` records a `keyword_enum!` set.

The derive runs a recorded constraint during validation and records it in the schema.
An editor's hover and completion read that same rule.
The `Validate` body carries no line for the field.

```rust
length_constraint!(HOSTNAME_LEN, max: 253);

#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(non_empty, length = HOSTNAME_LEN)]
    hostname: Located<String>,
    #[confval(range = PORT)]
    port: Located<i64>,
    #[confval(default, format = Ip)]
    peers: Vec<Located<String>>,
}
```

A recorded constraint expands where the spec struct is declared, so what it names must be reachable there.
The four attributes differ on what that means.
`range = ...` and `length = ...` name a value.
`range_constraint!` and `length_constraint!` generate a private const, so that const must be in the module that declares the spec struct.
`keywords = ...` and `format = ...` name a type, so the enum or the format type may be anywhere the spec module can import it from.
Hold every `keyword_enum!` in one vocabulary module and import it.
A closed set of words belongs to no single stage, because the spec checks it, lowering converts through it, and the runtime type holds it.
The `Validate` impl may also live in another module, because a trait impl can be anywhere in the crate.

The shape of the field decides whether a recorded constraint applies.
A `Located<T>` takes one, and so does an `Option<Located<T>>`, which the derive checks only when the value is present.
`keywords` also takes a string list, in both the bare and the optional form, where it records the set each element must come from and reports each bad element at its own span.
`format` takes a string list the same way, where each element must parse.
`range` takes a scalar leaf alone, because there is no numeric list shape for it to apply to.
A map and a nested block take neither.
The derive rejects an attribute on a shape that cannot carry it, so a misplaced attribute fails the build rather than being skipped in silence.

Write the check in `Validate` in three cases.

- A set of words held as a slice rather than as a `keyword_enum!` type, which `KeywordSet::new` checks.
- A spec with a handwritten `FromFields`, which has no derive to carry an attribute.
- A check that must not run under a gate, because `validate_all` runs every recorded check before `validate` and `descend` does not prune it.

The first two shapes have no attribute available, so the check has to go in `Validate`.
The third is your decision about whether the gate should suppress the message.

Write a `Validate` impl for the rules an attribute cannot express: a cross-field rule or a value with its own logic.
Its `validate` reports the rules for that type's own fields.
`validate_all` reaches the children, so a validator never calls a child's validator by hand.
Accumulate into the `Report` with no early return, so one run reports every problem.
Report at the offending field's span.

```rust
impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        if self.hostname.value == "0.0.0.0" {
            report
                .warning("hostname set to listen on every available network device")
                .at(self.hostname.span)
                .emit();
        }
    }
}
```

Leave numeric narrowing and keyword-to-enum conversion to lowering, where the `narrow` helpers report an out-of-range value that a rule missed.
Add a second location to a diagnostic with `.related(span, label)`.
For example, point a duplicate at the line that declared it first.

One case earns an early return.
When a block declares itself inapplicable, the rules below it check against the wrong thing and their diagnostics are noise.
A version field that names a schema this build does not know is the usual case.
Report the one error, return from `validate`, and override `descend` to break so the children are skipped too.

A `validate` impl sees only `&self`.
A rule that needs a sibling field's span, an enclosing span, or another file does not belong there.
Write it as a function that takes the surrounding values.
Call it yourself.

Give those functions one entry point that the loader calls.
It runs `validate_all` on the root, then each rule that spans more than one entity or more than one file.
Split them by scope as they grow, into the rules that hold within one document and the rules that hold across documents.
Keep the reusable leaf checks apart from both, because a check such as an address format or a path shape carries no domain rule of its own and several entities call it.

A rule may read local state when the answer helps the operator before startup.
Reading a file's contents, checking that a directory exists, and parsing a certificate are all fair.
Do not resolve a hostname, open a socket, or call a remote service.
A load that depends on remote state is slow and answers differently on each run.
Refuse the value at validation and resolve it at runtime instead.

Look for a relationship between two fields before you finish.
A pair of bounds that must be ordered, a field that is required only when another is set, and a total that must not exceed a limit are the common ones.
Report the rule at one field's span, and point at the other with `.related(span, label)`.
When the domain has no such relationship, write the empty `Validate` impl and say why.
An attribute records every field-local constraint, so a `Validate` impl that holds nothing else is empty by design rather than by omission.

Report a value as a warning when it is legal but likely wrong.
A value that is inside its range and still risky is the usual case.
A warning does not trip the gate, so the program starts and the operator reads the note.

### 5. Write the runtime type

The config is the resolved form.
It carries no `Located` wrapper and no `Option` on a field the runtime always needs.
Use `Arc<str>` for an identifier cloned on a hot path.
Derive the mapping with `#[derive(confval::Config)]` where it is mechanical, and write a `Lower` impl where lowering can fail in a way the derive cannot express.
A lowering function reports through the `Report` and returns `None` on failure rather than panicking, which is what the `narrow` helpers already do.

```rust
#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    hostname: String,
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    port: u16,
    #[confval(nested, default)]
    limits: LimitsConfig,
}
```

### 6. Wire the entry point

Run the phases in order: parse, validate, gate, then lower.
The runtime path never panics, so it holds no `unwrap` and no `expect`.
A misconfiguration is reported and the program exits or rejects the reload, rather than crashing a running service.
The gate is one call, `report.has_errors()`, and it must run before lowering.
Two things are the project's decision.
The first is the action when the gate trips, whether to exit or to reject a reload.
The second is whether a warning also stops the run.

```rust
let spec: Option<ServerSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);

// A syntax error yields None, with the reason already in the report. Handle it
// rather than unwrapping.
let Some(spec) = spec else {
    // render the report and stop, or reject the reload
    return;
};

spec.validate_all(&mut report);

// The gate. Stop before lowering while the report holds errors.
if report.has_errors() {
    // render the report and stop, or reject the reload
    return;
}

// A lowering error means a validation rule is missing. Report it, never panic.
if let Some(config) = ServerConfig::lower(&spec, &mut report) {
    // use config
}
```

A configuration that spans several files uses one `SourceMap` and one `Report` for the whole load.
A span carries the source it came from, so issues from different files merge into one report and render together.
Read and parse every file before you stop.
Record that a file failed to parse, and stop after the loop rather than at the first failure, so one run reports every syntax error.

Load once, at the outermost entry point that can report a failure and stop.
Do not move the load into a function that has no way to report one.
Give that function a resolved value instead.
A load pushed down into a function that returns nothing forces an error branch which cannot act.

Do not change the signature of a function the project already exports.
Add an entry point beside it that takes the configuration, and let the original delegate with a resolved default.
A changed signature moves the cost onto every existing caller, including the project's own tests.

Pass a consumer the narrowest part of the config it reads.
A function that needs one block takes that block, not the root.
The root belongs to the entry point that loaded it.
A wide parameter couples a module to settings it never touches, and it hides which settings that module depends on.
The new entry point is where this is easiest to get right, because nothing constrains its parameter list yet.

Return the diagnostics rather than print them.
The caller then owns the output stream, and a test asserts on the result rather than on captured standard error.

### 7. Write a starter configuration file

Write a file that names every setting at its default, and check it into the project.
An operator copies that file rather than reads the source to learn what the settings are.
Write it by hand, or generate it from the spec with `to_template`, which renders each field's doc comment above its field.
Say in the file which values are the defaults, and how a command line flag interacts with them.

### 8. Write a round-trip test

Cover each of these cases:

- A complete file lowers to the values it names.
- An omitted field takes its declared default.
- A file with several bad values reports every problem in one pass.
- A warning does not stop the load.
- No file at all yields the declared defaults.
- A syntax error reports rather than panics.

The third one is the property that makes accumulation worth having, so write it first.
A `[dependencies]` entry is available to test targets, so an integration test can import confval and assert on the `Report` directly.

Expose a way to build the configuration from specs rather than from text, and use it in the tests that do not exercise parsing.
`Located::detached` supplies a value with no source position.
A validator must not assume a source entry exists, because a detached span has none.

```rust
#[test]
fn a_bad_fixture_reports_every_problem_at_once() {
    // Arrange
    let text = "hostname = \"\"\nport = 99999\n";
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("server.toml", text);

    // Act
    let spec = confval::format::toml::parse_toml(&sources, id, &mut report);
    if let Some(spec) = &spec {
        spec.validate_all(&mut report);
    }

    // Assert
    // The empty hostname and the out-of-range port are both reported, so the
    // report carries more than one issue rather than stopping at the first.
    assert!(report.has_errors());
    assert!(report.issues().len() > 1);
}
```

### 9. Verify

Run `cargo check`, then `cargo test`.
Read the spec, validation, and lowering back against the current confval crate.
`cargo check` catches a renamed type.
It does not catch advice that is wrong.

## Provenance

This skill was written for confval {{confval_version}}.
