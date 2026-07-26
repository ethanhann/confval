use confval::source::Located;

// `#[confval(derive_default)]` cannot derive a value for a non-optional leaf
// with no attribute default, so it names the field and refuses rather than
// inventing `String::default()`.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct ServerSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    hostname: Located<String>,
}

fn main() {}
