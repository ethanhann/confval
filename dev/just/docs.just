# Launch the docs site locally
docs:
    cd docs && npm run start

# Regenerate the dark variant of an architecture SVG from its light source
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

validate-docs: check-doc-snippets check-doc-programs
    cargo doc --all-features --no-deps
    cd docs && npm run build

# Compile every Rust fence in the docs that contains fn main
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

# Fail if RustRover mangled a Rust code fence in the docs: a spread borrow ("& x", "( &x"), or a method chain reflowed to column zero
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
