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
    cargo run -q -p confval --features derive,color,hcl --example hcl
    cargo run -q -p confval --features derive,color,toml --example toml
    cargo run -q -p confval --features derive,color,toml --example issue_severity
    cargo run -q -p confval --features derive,color,toml --example validate_traversal
    cargo run -q -p confval --features derive,color,toml,layering --example layering
    cargo run -q -p confval --features derive,color,toml,hcl --example templates
    cargo run -q -p confval --features derive,color,kdl --example kdl
    cargo run -q -p confval --features derive,toml --example doc_fallback
    cargo run -q -p confval --features derive,serde,toml --example json_diagnostics
    cargo run -q -p confval --features derive,color,toml --example narrow
    cargo run -q -p confval --features derive,serde,toml --example representations

validate-docs: check-doc-snippets check-doc-programs
    cargo doc --all-features --no-deps
    cd docs && npm run build

# Compile every Rust fence in the docs that contains fn main, so a full-program listing cannot drift from the crate.
check-doc-programs:
    #!/usr/bin/env bash
    set -euo pipefail
    dir=target/doc-programs
    rm -rf "$dir"
    mkdir -p "$dir/src/bin"
    for f in $(find docs/docs docs/releases -name '*.md'); do
      prefix=$(basename "$f" .md | tr -c 'a-zA-Z0-9' '_')
      awk -v outdir="$dir/src/bin" -v prefix="$prefix" '
        /^```rust$/ { inblock = 1; buf = ""; next }
        /^```/ { if (inblock) { inblock = 0; if (buf ~ /fn main/) { count++; printf "%s", buf > (outdir "/" prefix count ".rs") } } next }
        inblock { buf = buf $0 "\n" }
      ' "$f"
    done
    count=$(ls "$dir/src/bin" | wc -l | tr -d ' ')
    if [ "$count" -eq 0 ]; then echo "no full-program doc snippets found"; exit 0; fi
    printf '[package]\nname = "doc-programs"\nversion = "0.0.0"\nedition = "2024"\n\n[dependencies]\nconfval = { path = "../../crates/confval", features = ["derive", "toml", "hcl", "color", "serde", "layering"] }\n\n[workspace]\n' > "$dir/Cargo.toml"
    cargo build -q --manifest-path "$dir/Cargo.toml"
    echo "compiled $count doc program(s)"

# Fail if RustRover mangled a Rust code fence in the docs: a spread borrow ("& x", "( &x"), or a method chain reflowed to column zero.
check-doc-snippets:
    #!/usr/bin/env bash
    set -euo pipefail
    files=$(find docs/docs docs/releases -name '*.md')
    [ -z "$files" ] && exit 0
    hits=$(awk '
      FNR == 1 { inrust = 0; incode = 0 }
      /^```rust/ { inrust = 1; incode = 1; next }
      /^```/ { if (incode) { incode = 0; inrust = 0 } else { incode = 1 } next }
      inrust && ($0 ~ /[^&]& / || $0 ~ /^& / || $0 ~ /\( &/ || $0 ~ /^\./) { printf "%s:%d:%s\n", FILENAME, FNR, $0 }
    ' $files)
    if [ -n "$hits" ]; then
      echo "Mangled Rust code fences (RustRover reformatting?):" >&2
      echo "$hits" >&2
      echo "Fix: change '& x' to '&x', '( &x' to '(&x', and indent a leading '.method' chain line." >&2
      exit 1
    fi

# Publish both crates to crates.io, confval-derive first (confval pins it with `=`).
publish:
    cargo publish -p confval-derive
    cargo publish -p confval

docs:
    cd docs && npm run start
