use confval::source::Located;

// A non-optional nested block with no default has no value to derive, so
// `derive_default` refuses it the same way it refuses a leaf or a list. A bare
// `#[confval(nested, default)]` fills it with `Inner::default()` and resolves
// the error.
#[derive(confval::Spec)]
struct Inner {
    #[confval(default = 7)]
    n: Located<i64>,
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct ServerSpec {
    #[confval(default = 16)]
    max_body_mb: Located<i64>,
    #[confval(nested)]
    inner: Located<Inner>,
}

fn main() {}
