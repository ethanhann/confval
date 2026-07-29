use confval::prelude::*;

// A key repeated inside one attribute was silently last-wins. It must be
// rejected like an unknown key is.
#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(default = 1, default = 2)]
    port: Located<i64>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
