use confval::prelude::*;

// A typo in a struct-level key must be a curated error, matching the Config
// derive's equivalent fixture.
#[derive(confval::Spec)]
#[confval(derive_defalt)]
struct ServerSpec {
    port: Located<i64>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
