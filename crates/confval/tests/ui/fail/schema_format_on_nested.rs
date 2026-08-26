use confval::prelude::*;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(nested, format = Ipv4)]
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
