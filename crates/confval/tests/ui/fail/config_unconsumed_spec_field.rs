use confval::diagnostic::Report;
use confval::pipeline::Validate;
use confval::source::Located;

#[derive(confval::Spec)]
struct ServerSpec {
    port: Located<i64>,
    threads: Located<i64>,
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
