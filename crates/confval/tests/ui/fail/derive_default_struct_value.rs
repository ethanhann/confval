use confval::prelude::Located;

// The struct-level flag takes no value.
#[derive(confval::Spec)]
#[confval(derive_default = true)]
struct Spec {
    #[confval(default)]
    size: Located<i64>,
}

fn main() {}
