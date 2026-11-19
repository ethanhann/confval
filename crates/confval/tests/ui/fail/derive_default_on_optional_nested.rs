use confval::prelude::{Located, Report, Validate};

#[derive(confval::Spec)]
#[confval(derive_default)]
struct Child {
    #[confval(default)]
    size: Located<i64>,
}

impl Validate for Child {
    fn validate(&self, _report: &mut Report) {}
}

// An optional nested block already generates `None`, so a generated value has
// no meaning.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(nested, derive_default = Child::default())]
    child: Option<Located<Child>>,
}

fn main() {}
