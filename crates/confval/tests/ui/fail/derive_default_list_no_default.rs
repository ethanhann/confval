use confval::source::Located;

// A required string list has no attribute default, so `derive_default` refuses
// it the same way it refuses a leaf. Adding a bare `#[confval(default)]` makes
// the list default to empty and resolves the error.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct ServerSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    tags: Vec<Located<String>>,
}

fn main() {}
