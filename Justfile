#!/usr/bin/env just --justfile

# List recipes
list:
    just --list

# Default recipe
default: list

# Import modular justfiles

import "dev/just/code_quality.just"
import "dev/just/docs.just"
import "dev/just/examples.just"
import "dev/just/profile.just"
import "dev/just/publish.just"
import "dev/just/test.just"

# Test everything
validate: format lint check-code-quality check-frontends check-bin check-lsp-example test validate-docs examples
