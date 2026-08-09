#!/usr/bin/env just --justfile

release:
    cargo build --release    

test:
    cargo nextest run --workspace --all-features
    cargo test --locked --all-features --doc

# Run unit tests with coverage, and output HTML
test-with-coverage:
    cargo llvm-cov nextest --workspace --exclude confval-derive --all-features --html --ignore-filename-regex 'tests/|examples/'
    open target/llvm-cov/html/index.html

format:
    cargo fmt

lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run mutation testing across the workspace. Configured in .cargo/mutants.toml.
mutants jobs="4":
    cargo mutants -j {{ jobs }}

# Mutate only the source a diff touches. Note the jobs="4" arg is bound by disk (target / size * jobs), not CPU cores.
mutants-diff base="main" jobs="4":
    #!/usr/bin/env bash
    set -euo pipefail
    diff=target/mutants-since.diff
    mkdir -p target
    git diff {{ base }} -- 'crates/*/src/*.rs' > "$diff"
    if [ ! -s "$diff" ]; then echo "no source changes against {{ base }}"; exit 0; fi
    cargo mutants --in-diff "$diff" -j {{ jobs }}

# Compile each format frontend alone, so a cfg gate the all-features build hides cannot drift.
check-frontends:
    cargo check -q -p confval --no-default-features --features derive,hcl
    cargo check -q -p confval --no-default-features --features derive,toml
    cargo check -q -p confval --no-default-features --features derive,kdl
    cargo check -q -p confval --no-default-features --features derive,json
    cargo check -q -p confval --no-default-features --features derive,yaml

# Test everything
validate: format lint check-frontends test validate-docs examples

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
    cargo run -q -p confval --features derive,color,json --example json
    cargo run -q -p confval --features derive,color,yaml --example yaml
    cargo run -q -p confval --features derive,toml --example doc_fallback
    cargo run -q -p confval --features derive,serde,toml --example json_diagnostics
    cargo run -q -p confval --features derive,color,toml --example narrow
    cargo run -q -p confval --features derive,serde,toml --example representations
    cargo run -q -p confval --features derive,color,toml,hcl --example handwritten

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
