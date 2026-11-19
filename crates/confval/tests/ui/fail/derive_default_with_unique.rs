use confval::prelude::Located;

// The macro cannot prove that an arbitrary generated list has no duplicate
// value, so it refuses the pair rather than allowing a default that fails the
// check.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(unique, derive_default = vec![Located::detached("x".to_string())])]
    names: Vec<Located<String>>,
}

fn main() {}
