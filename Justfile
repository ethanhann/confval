#!/usr/bin/env just --justfile

release:
  cargo build --release    

test:
  cargo nextest run --workspace --all-features

lint:
  cargo clippy

# Publish both crates to crates.io, confval-derive first (confval pins it with `=`).
publish:
    cargo publish -p confval-derive
    cargo publish -p confval

docs:
    cd docs && npm run start