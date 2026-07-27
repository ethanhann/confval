use confval::prelude::{Located, Report, Validate};

// A `#[confval(nested, default)]` marker fills an absent block from its inner
// type's `Default`, so the inner type must implement `Default`. An inner type
// with no `Default` is a compile error pointing at the missing bound.
#[derive(confval::Spec)]
struct InnerSpec {
    name: Located<String>,
}

impl Validate for InnerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(nested, default)]
    inner: Option<Located<InnerSpec>>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
