# Profiling with hotpath. Every recipe enables the `hotpath` feature on the
# command line, so a plain build stays free of it.

# The features each profiled example needs, before the hotpath feature.
profile_features_toml := "derive,color,toml"
profile_features_layering := "derive,color,toml,layering"

[private]
profile-run example extra_features cargo_flags:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ example }}" in
      toml) features="{{ profile_features_toml }}" ;;
      layering) features="{{ profile_features_layering }}" ;;
      *) echo "unknown profile target: {{ example }} (expected toml or layering)" >&2; exit 1 ;;
    esac
    cargo run -q -p confval {{ cargo_flags }} --features "$features,hotpath{{ extra_features }}" --example {{ example }}

# Profile the parse, validate, lower, and emit path. Pass `layering` for the layered assembly path.
profile example="toml":
    just profile-run {{ example }} "" ""

# Profile with allocation counts as well as durations
profile-alloc example="toml":
    just profile-run {{ example }} ",hotpath-alloc" ""

# Profile an optimized build, which reports representative durations
profile-release example="toml":
    just profile-run {{ example }} "" "--release"

# Profile and write the report as JSON for a tool to read
profile-json example="toml":
    HOTPATH_OUTPUT_FORMAT=json just profile-run {{ example }} "" "--release"

# Profile the language server router: a scripted LSP session over an in-memory connection
profile-lsp:
    cargo run -q -p confval-lsp --example profile_router --features hcl,hotpath

# Profile the language server router with allocation counts
profile-lsp-alloc:
    cargo run -q -p confval-lsp --example profile_router --features hcl,hotpath,hotpath-alloc

# Profile the language server router in an optimized build
profile-lsp-release:
    cargo run -q --release -p confval-lsp --example profile_router --features hcl,hotpath

# Attach the live hotpath TUI to a running profiled process (port 6770)
profile-console:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v hotpath >/dev/null; then
      echo "hotpath CLI not found. Install it with: cargo install hotpath --features tui" >&2
      exit 1
    fi
    hotpath console

# Confirm profiling stays optional: build with the feature off, then on
profile-check:
    cargo check --workspace --all-targets
    cargo check -p confval --all-targets --features derive,color,toml,hcl,kdl,json,yaml,layering,serde,hotpath
    cargo check -p confval-lsp --all-targets --features hotpath
