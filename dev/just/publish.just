# Publish the crates to crates.io in dependency order (each is pinned with `=`).
publish:
    cargo publish -p confval-derive
    cargo publish -p confval
    cargo publish -p confval-lsp
