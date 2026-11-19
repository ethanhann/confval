use confval::source::Located;

// An absent optional field is already `None` in the generated impl, so a
// value from `derive_default` has no meaning.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(derive_default = 1)]
    count: Option<Located<i64>>,
}

fn main() {}
