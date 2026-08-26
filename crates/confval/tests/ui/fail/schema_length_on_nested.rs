use confval::prelude::*;

length_constraint!(NAME_LEN, min: 1, max: 63);

#[derive(confval::Spec)]
struct Cfg {
    #[confval(nested, length = NAME_LEN)]
    child: Located<Child>,
}

#[derive(confval::Spec)]
struct Child {
    name: Located<String>,
}

impl Validate for Child {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
