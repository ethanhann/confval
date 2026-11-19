use confval::prelude::Located;

// An optional wrapped list already generates `None`, so a generated value has
// no meaning.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(derive_default = vec![Located::detached("a".to_string())])]
    names: Option<Located<Vec<Located<String>>>>,
}

fn main() {}
