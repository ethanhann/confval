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

# Enforce the workspace line-coverage floor. Slow, so it runs here and in pre-release rather than in validate.
check-coverage:
    cargo llvm-cov nextest --workspace --exclude confval-derive --all-features --ignore-filename-regex 'tests/|examples/|confval-derive/' --fail-under-lines 95

format:
    cargo fmt

lint:
    cargo clippy --all-targets --all-features -- -D warnings -D clippy::cognitive_complexity

# Check hygiene the lint recipe does not: duplication, formatting, unused deps, and file size. Run by validate.
check-code-quality:
    #!/usr/bin/env bash
    set -euo pipefail
    # The near cap admits two structural cousins. The derive's default rendering
    # and the populate walk's leaf mapping both dispatch per leaf and produce
    # different tokens. `KeywordSet::new` and `SchemaType::string_list` are
    # one-line constructors in different crates that wrap unrelated values.
    cargo dupes --exclude tests --exclude benches --exclude examples --exclude-tests check --max-exact 32 --max-near 6 --max-exact-percent 5.0 --max-near-percent 1.2
    cargo machete
    cargo fmt --check
    fail=0
    while IFS= read -r f; do
      line=$(grep -n '#\[cfg(test)\]\|#\[cfg(all(test' "$f" | head -1 | cut -d: -f1 || true)
      if [ -n "$line" ]; then app=$((line - 1)); else app=$(wc -l < "$f"); fi
      if [ "$app" -gt 600 ]; then echo "file over the 600 application-line hard limit: $f ($app lines)" >&2; fail=1; fi
    done < <(find crates -path '*/src/*' -name '*.rs')
    [ "$fail" -eq 0 ] || exit 1
    echo "check-code-quality passed"

# Fast mutant test args: skip the trybuild `compile_fail` case, which costs ~10s
# per run and cannot catch a runtime-logic mutant. Keep it for `mutants-derive`,
# where the derive macro's compile-error paths need it.
mutants_fast_args := "--profile mutants --cargo-test-arg -E --cargo-test-arg 'not binary(compile_fail)'"

# Run mutation testing across the workspace. Configured in .cargo/mutants.toml.
mutants jobs="6":
    cargo mutants {{ mutants_fast_args }} -j {{ jobs }}

# Mutate only the source a diff touches. The mutants profile drops debug info, so the disk cost per job is lower; raise jobs as disk allows.
mutants-diff base="main" jobs="6":
    #!/usr/bin/env bash
    set -euo pipefail
    diff=target/mutants-since.diff
    mkdir -p target
    git diff {{ base }} -- 'crates/*/src/*.rs' > "$diff"
    if [ ! -s "$diff" ]; then echo "no source changes against {{ base }}"; exit 0; fi
    cargo mutants --in-diff "$diff" {{ mutants_fast_args }} -j {{ jobs }}

# Mutate only confval-derive, with the trybuild `compile_fail` test kept in. Run this to cover the derive macro's diagnostic paths that the fast recipes skip.
mutants-derive jobs="4":
    cargo mutants -p confval-derive --profile mutants -j {{ jobs }}

# Compile each format frontend alone, so a cfg gate the all-features build hides cannot drift.
check-frontends:
    cargo check -q -p confval --no-default-features --features derive,hcl
    cargo check -q -p confval --no-default-features --features derive,toml
    cargo check -q -p confval --no-default-features --features derive,kdl
    cargo check -q -p confval --no-default-features --features derive,json
    cargo check -q -p confval --no-default-features --features derive,yaml
    cargo check -q -p confval-lsp --no-default-features --features hcl
    cargo check -q -p confval-lsp --no-default-features --features toml
    cargo check -q -p confval-lsp --no-default-features --features kdl
    cargo check -q -p confval-lsp --no-default-features --features json
    cargo check -q -p confval-lsp --no-default-features --features yaml

# Check the bin compiles under the empty default feature set, so it stays free of a feature dependency.
check-bin:
    cargo check -p confval --no-default-features --bin confval

# Check the LSP example servers
check-lsp-example:
    cargo check -p confval-lsp --example serve
    cargo check -p confval-lsp --example serve_multi --no-default-features --features hcl

# Test everything
validate: format lint check-code-quality check-frontends check-bin check-lsp-example test validate-docs examples

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
    for f in $(find docs/docs docs/releases crates/confval/skills -name '*.md'); do
      # Derive the prefix from the whole path, not the basename, so two files
      # sharing a basename (both SKILL.md, references/pipeline.md and docs/docs/pipeline.md)
      # get distinct output files instead of one silently overwriting the other.
      prefix=$(echo "$f" | tr -c 'a-zA-Z0-9' '_')
      awk -v outdir="$dir/src/bin" -v prefix="$prefix" '
        /^```rust$/ { inblock = 1; buf = ""; next }
        /^```/ { if (inblock) { inblock = 0; if (buf ~ /fn main/) { count++; printf "%s", buf > (outdir "/" prefix count ".rs") } } next }
        inblock { buf = buf $0 "\n" }
      ' "$f"
    done
    count=$(ls "$dir/src/bin" | wc -l | tr -d ' ')
    if [ "$count" -eq 0 ]; then echo "no full-program doc snippets found"; exit 0; fi
    printf '[package]\nname = "doc-programs"\nversion = "0.0.0"\nedition = "2024"\n\n[dependencies]\nconfval = { path = "../../crates/confval", features = ["derive", "toml", "hcl", "kdl", "json", "yaml", "color", "serde", "layering"] }\n\n[workspace]\n' > "$dir/Cargo.toml"
    cargo build -q --manifest-path "$dir/Cargo.toml"
    echo "compiled $count doc program(s)"

# Fail if RustRover mangled a Rust code fence in the docs: a spread borrow ("& x", "( &x"), or a method chain reflowed to column zero.
check-doc-snippets:
    #!/usr/bin/env bash
    set -euo pipefail
    files=$(find docs/docs docs/releases crates/confval/skills -name '*.md')
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

# Publish the crates to crates.io in dependency order (each is pinned with `=`).
publish:
    cargo publish -p confval-derive
    cargo publish -p confval
    cargo publish -p confval-lsp

docs:
    cd docs && npm run start

# Regenerate the dark variant of an architecture SVG from its light source, recoloring text, connectors, and fills for dark mode.
docs-diagram-dark name="high_level_architecture":
    #!/usr/bin/env bash
    set -euo pipefail
    cd docs/static/img
    cp "{{name}}.svg" "{{name}}_dark.svg"
    perl -0777 -pi \
      -e 's/<svg /<svg fill="#c6d3c6" /;' \
      -e 's/"#3a414a"/"#c6d3c6"/g;' \
      -e 's/"#333"/"#c6d3c6"/g;' \
      -e 's/"#010000"/"#c6d3c6"/g;' \
      -e 's/"#fff"/"#10160f"/g;' \
      -e 's/"#f2f3f5"/"#131c15"/g;' \
      "{{name}}_dark.svg"
