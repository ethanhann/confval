#!/usr/bin/env just --justfile

release:
    cargo build --release    

test:
    cargo nextest run --workspace --all-features
    cargo test --locked --all-features --doc

format:
    cargo fmt

lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Test everything
validate: format lint test validate-docs examples

# Run examples
examples:
    echo "Running crate examples..."
    -cargo run -q -p confval --features derive,color,hcl,serde --example hcl
    cargo run -q -p confval --features derive,color,toml,serde --example toml
    cargo run -q -p confval --features derive,color,toml,serde --example issue_severity
    cargo run -q -p confval --features derive,color,toml,serde --example validate_traversal
    cargo run -q -p confval --features derive,color,toml,serde,layering --example layering
    cargo run -q -p confval --features derive,color,toml,serde --example populate

validate-docs: check-doc-snippets
    cargo doc --all-features --no-deps
    cd docs && npm run build

# Fail if RustRover mangled a borrow (e.g. "& x" or "& mut") inside a Rust code fence in the docs.
check-doc-snippets:
    #!/usr/bin/env bash
    set -euo pipefail
    files=$(find docs/docs docs/releases -name '*.md')
    [ -z "$files" ] && exit 0
    hits=$(awk '
      FNR == 1 { inrust = 0; incode = 0 }
      /^```rust/ { inrust = 1; incode = 1; next }
      /^```/ { if (incode) { incode = 0; inrust = 0 } else { incode = 1 } next }
      inrust && ($0 ~ /[^&]& / || $0 ~ /^& /) { printf "%s:%d:%s\n", FILENAME, FNR, $0 }
    ' $files)
    if [ -n "$hits" ]; then
      echo "Mangled borrows in Rust code fences (RustRover reformatting?):" >&2
      echo "$hits" >&2
      echo "Fix: change '& x' to '&x' and '& mut' to '&mut'." >&2
      exit 1
    fi

# Publish both crates to crates.io, confval-derive first (confval pins it with `=`).
publish:
    cargo publish -p confval-derive
    cargo publish -p confval

docs:
    cd docs && npm run start
