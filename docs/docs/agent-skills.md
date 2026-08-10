---
sidebar_position: 5
---

# Agent Skills

Setting up a confval pipeline is mechanical and specific to your domain model.
You write a spec type whose fields carry a span, validation that accumulates into a report, and lowering into runtime types, and the shape of each depends on the settings you parse.
confval ships two agent skills that walk an agent through that work, and a `confval` binary whose job is to install them into your project.

The binary parses no configuration and validates nothing.
It writes the skill files and reports what it wrote.

## Installing the binary

The binary lives in the `confval` package, so the crate you already depend on installs it.

```shell
cargo install confval
```

`confval init` then writes the skills into your project.

## The two skills

The skills answer two different questions.

`confval-init` scaffolds a pipeline in a project that does not have one.
It surveys the project, reads the configuration format, adds the dependency, and writes the spec, validation, and runtime layers, stopping at the boundaries your domain rules begin.

`confval-add-block` keeps the layers in sync when you add a field or block to a project that already has a pipeline.
A new setting touches the spec type, the validation, the runtime type, the lowering, and the `Default` impl, and the skill's job is that none of the five is missed.

The skills are written to disk rather than injected into one session, because `confval-add-block` is a maintenance procedure you need long after anyone ran `confval init`.

## Running confval init

```shell
confval init
```

The default writes both skills into the project, prints one line per file, and exits.
Project scope is the default, because the skills describe one project's configuration layer, and a project skill can be committed so everyone on the repository has it.

The binary installs into the repository root, which it finds by walking up from the working directory to the nearest ancestor that holds a `.git` entry.
The walk is what makes the command usable from anywhere in a repository.
The report names the absolute directory it chose, so the walk is visible rather than silent.

### Where the files land

The agent selects the directory segment and the scope selects the base.
`--agent claude` selects `.claude`, the only segment this release writes.

| Scope                   | Base                | Path written                             |
|-------------------------|---------------------|------------------------------------------|
| `project` (default)     | the repository root | `<root>/.claude/skills/<skill>/SKILL.md` |
| `user` (`--scope user`) | your home directory | `<home>/.claude/skills/<skill>/SKILL.md` |

A reference file lands beside its `SKILL.md` at its relative path.

### Invoking a skill

For a project or personal skill, the invocation comes from the directory name.
After `confval init`, run `claude` in the project and invoke `/confval-init` or `/confval-add-block`.
Pass `--launch` to `confval init` to open a primed session for you.

```shell
confval init --launch
```

### Listing the skills

`confval init --list` prints each skill and its description and writes nothing.

## Outcomes and exit codes

Each file gets one outcome, decided by comparing the bytes on disk with the bytes the binary would write.

| Situation                               | Outcome     |
|-----------------------------------------|-------------|
| no file at the path                     | `created`   |
| the file matches what the binary writes | `unchanged` |
| the file differs, without `--force`     | `skipped`   |
| the file differs, with `--force`        | `updated`   |

The version of confval the binary was built from is substituted into the skill text when it writes, so an upgraded binary reports an untouched older file as differing.
That is the drift signal, and `--force` is how you take the newer text.
The report describes the file as differing from the copy the binary ships rather than as edited, because an older binary's output differs for the same reason.

| Code | Meaning                                                                       |
|------|-------------------------------------------------------------------------------|
| 0    | every file is present and current, and the agent exited 0 if one was launched |
| 1    | at least one file was skipped                                                 |
| 2    | a usage error, including no subcommand, an unknown flag, agent, or scope      |
| 3    | an IO error, or an agent that could not be launched                           |
| 4    | the agent ran and exited non-zero                                             |

## What you can observe

A first run reports each file `created`.
A second run reports `unchanged` and leaves the bytes alone.
A file you edited reports as differing from the copy the binary ships and is left alone until you pass `--force`, and an upgraded binary reports the same way for a file it did not write.

Nothing is ever deleted.
A reference file a later release stops shipping stays on disk until you remove it, and a file already in a skill directory that the binary does not ship is left alone and not reported.
