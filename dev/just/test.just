# Run workspace tests
test:
    cargo nextest run --workspace --all-features
    cargo test --locked --all-features --doc

# Run unit tests with coverage, and output HTML
test-with-coverage:
    cargo llvm-cov nextest --workspace --exclude confval-derive --all-features -E 'not binary(compile_fail)' --html --ignore-filename-regex 'tests/|examples/'
    open target/llvm-cov/html/index.html

# Fast mutant test args: skip the trybuild `compile_fail` case, which costs ~10s per run
mutants_fast_args := "--profile mutants --cargo-test-arg -E --cargo-test-arg 'not binary(compile_fail)'"

# Run mutation testing across the workspace. Configured in .cargo/mutants.toml.
mutants jobs="6":
    cargo mutants {{ mutants_fast_args }} -j {{ jobs }}

# Mutate only the source a diff touches (much faster than a full mutants recipe run)
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
