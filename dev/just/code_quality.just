# Enforce the workspace line-coverage floor
check-coverage:
    cargo llvm-cov nextest --workspace --exclude confval-derive --all-features -E 'not binary(compile_fail)' --ignore-filename-regex 'tests/|examples/|confval-derive/' --fail-under-lines 95

format:
    cargo fmt

lint:
    cargo clippy --all-targets --all-features -- -D warnings -D clippy::cognitive_complexity

# Check hygiene behond thle lint recipe: duplication, formatting, unused deps, and file size
check-code-quality:
    #!/usr/bin/env bash
    set -euo pipefail
    # The near cap admits two structural cousins. The derive's default rendering
    # and the populate walk's leaf mapping both dispatch per leaf and produce
    # different tokens. `KeywordSet::new` and `SchemaType::string_list` are
    # one-line constructors in different crates that wrap unrelated values.
    cargo dupes --exclude tests --exclude benches --exclude examples --exclude-tests check --max-exact 38 --max-near 8 --max-exact-percent 5.0 --max-near-percent 1.2
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

# Compile each format frontend alone to detect feature-related issues
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

# Check the bin compiles under the empty default feature set to make sure it stays free of a feature dependency
check-bin:
    cargo check -p confval --no-default-features --bin confval

# Check the LSP example servers
check-lsp-example:
    cargo check -p confval-lsp --example serve
    cargo check -p confval-lsp --example serve_multi --no-default-features --features hcl
