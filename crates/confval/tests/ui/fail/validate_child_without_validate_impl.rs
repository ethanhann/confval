use confval::diagnostic::Report;
use confval::pipeline::Validate;
use confval::source::Located;

#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(nested)]
    limits: Located<LimitsSpec>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

// No `impl Validate for LimitsSpec`. The traversal that `#[derive(Spec)]`
// generates for `ServerSpec` calls `validate_all` on this child. The missing
// validator is therefore a compile error at the parent rather than a block
// that is quietly never checked.
#[derive(Default, confval::Spec)]
struct LimitsSpec {
    max_body_mb: Located<i64>,
}

fn main() {}
