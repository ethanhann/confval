---
sidebar_position: 10
---

# Editor Support

You can see configuration errors in your editor before you run the program.
When your editor is connected to a confval language server, it shows which fields are legal, what each one holds, and where the file is wrong.

The [Language Server](./language-server.md) page covers how a developer runs one.
This page describes what the editor does for you once it is running.

## Diagnostics

The editor underlines the same errors your program would report.
It runs the real validation rather than an approximation.
An error the editor shows is an error the program would raise.
The checks include an unknown field, an out-of-range value, an undefined reference, a duplicate label, and an empty label.

## Completion

Completion offers what is legal at the cursor.

On a fresh line inside a block, it offers the field names and block types the schema allows there.
It hides a single-valued field you have already set.
The list follows the schema's declaration order, so related fields stay together.

On a value, it offers the values the schema allows.
A field with a fixed set of keywords offers those keywords.
Inside a list whose elements come from such a set, it offers the same words.
Accepting one replaces the element under the cursor rather than the whole list.
A field that points at another block offers the labels defined in the scope the reference resolves in.
These are the labels a reference can resolve to without an error.

Completion keeps working while you type, before the file is valid.

## Defaults

Accepting a field that has a default writes the default in for you.
The value arrives selected, so one keystroke replaces it.
For example, accepting `workers` writes `workers = 4` with the `4` selected.
On a value position, the default is offered as a preselected item.

## Hover

Hover on a field reads its documentation.
It shows the field's doc comment, its type, its constraint, and whether it has a default.
A field marked `#[confval(non_empty)]` reads "Must not be empty."
A list marked `#[confval(unique)]` reads "Entries must be unique."
A field with a length bound reads `Between {min} and {max} characters.`
A bound that starts at zero reads `At most {max} characters.`
A field with a format reads "Format: IPv4 address."
It also states whether the configuration sets the field or leaves it to the default.
For example, hover on `workers` reads "Defaults to 4."
A default the editor cannot print as a value, such as a list, states only that a default applies.
Hover on a reference value names the block it points to and states whether the value matches a defined label.

## Quick fixes

A value that has a default carries a quick fix.
The fix sets the field to its default.
For example, an out-of-range `workers = 9999` becomes `workers = 4` in one step.

## Navigation

Navigation follows references and labels.
Go-to-definition jumps from a reference value to the label it names.
Find-references lists every reference to a label, whether you start from the label or from one of its references.
The editor's outline and breadcrumbs show the block tree, with each block carrying its label.

## Formats

The editor supports every format confval parses: HCL, TOML, KDL, JSON, and YAML.

For HCL, TOML, KDL, and JSON, completion and navigation stay precise while you type, even before the file parses.
YAML reads structure from indentation, so it handles the common shapes, including a block sequence, a value on the next line, and an inline flow collection.
An unusual YAML layout, such as a flow collection spread across several lines, can resolve less precisely than a block mapping.

## One open file at a time

The editor checks the file you have open on its own.
When you assemble a configuration from several [layers](./layering.md), a later layer can supply a value the open file leaves out.
The editor does not see the other layers.
It can report a required field as missing even though a layer supplies it.
In a layered setup, treat a missing-field error as a prompt to check the other layers rather than a fault in the open file.

A multi document server holds every file of the configuration in one process, but each open file is still checked on its own.
