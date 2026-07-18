#!/usr/bin/env just --justfile

release:
  cargo build --release    

test:
  cargo nextest run --workspace --all-features

lint:
  cargo clippy

# Publish confval to crates.io
publish-confval:
    cargo publish -p confval

docs:
    cd docs && npm run start