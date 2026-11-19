use confval::prelude::Located;

// The macro cannot prove that an arbitrary generated value is non-empty, so it
// refuses the pair rather than allowing a default that fails the check.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(non_empty, derive_default = "x".to_string())]
    name: Located<String>,
}

fn main() {}
