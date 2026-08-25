# Run examples to show their output (handy for spot checking issues)
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
