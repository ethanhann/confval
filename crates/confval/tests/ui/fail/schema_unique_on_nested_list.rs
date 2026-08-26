use confval::prelude::*;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(nested, unique)]
    kids: Vec<Located<Child>>,
}

#[derive(confval::Spec)]
struct Child {
    name: Located<String>,
}

impl Validate for Child {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
