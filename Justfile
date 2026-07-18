#!/usr/bin/env just --justfile

release:
  cargo build --release    

lint:
  cargo clippy

# Publish confval to crates.io
publish-confval:
    cargo publish -p confval
