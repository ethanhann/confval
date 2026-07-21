use confval::source::Located;

#[derive(confval::Spec)]
struct ServerSpec {
    port: Located<i64>,
}

// No `impl Validate for ServerSpec`. Every generated `Lower` impl carries the
// bound, so the config fails to compile.

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    port: i64,
}

fn main() {}
