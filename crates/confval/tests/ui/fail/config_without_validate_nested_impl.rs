use confval::diagnostic::Report;
use confval::pipeline::Validate;
use confval::source::Located;

// A spec that parses itself rather than deriving `Spec`. The traversal that
// the derive would have generated does not exist, so the lowering bound
// reports it. The fix is an explicit `impl ValidateNested`. Writing one forces
// an answer to how this type's children are validated.
struct ServerSpec {
    port: Located<i64>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
struct ServerConfig {
    port: i64,
}

fn main() {}
