use confval::diagnostic::Report;
use confval::pipeline::Validate;
use confval::source::Located;
use core::ops::ControlFlow;

#[derive(Default, confval::Spec)]
struct ServerSpec {
    #[confval(nested)]
    limits: Located<LimitsSpec>,
    #[confval(nested)]
    tls: Option<Located<TlsSpec>>,
    #[confval(nested)]
    routes: Vec<Located<RouteSpec>>,
}

impl Validate for ServerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(Default, confval::Spec)]
struct LimitsSpec {
    max_body_mb: Located<i64>,
}

/// An empty impl still gets the generated traversal. This one reaches nothing,
/// because the type has no nested fields.
impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(Default, confval::Spec)]
struct TlsSpec {
    cert: Located<String>,
}

impl Validate for TlsSpec {
    fn validate(&self, _report: &mut Report) {}

    fn descend(&self) -> ControlFlow<()> {
        ControlFlow::Break(())
    }
}

#[derive(Default, confval::Spec)]
struct RouteSpec {
    path: Located<String>,
}

impl Validate for RouteSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {
    let spec = ServerSpec::default();
    let mut report = Report::new();
    spec.validate_all(&mut report);
}
