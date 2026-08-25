//! The fixture spec the handlers are tested against.
//!
//! It mirrors the `common` example spec, the representative shape the schema IR
//! is pinned against: a required scalar, a defaulted scalar with a range, a
//! keyword field, a string list, a constrained string list in both shapes, a
//! map, an optional nested block, and a repeated block.
#![allow(dead_code)]

use confval::prelude::*;
use std::collections::BTreeMap;

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

/// The root spec of the fixture.
#[derive(confval::Spec)]
pub struct ServerSpec {
    /// The address the server binds.
    #[confval(non_empty)]
    pub hostname: Located<String>,
    /// The TCP port the server listens on.
    #[confval(range = PORT)]
    pub port: Located<i64>,
    /// The number of worker threads.
    #[confval(default = 4, range = WORKERS)]
    pub workers: Located<i64>,
    /// Whether to serve TLS.
    #[confval(default = false)]
    pub tls: Located<bool>,
    /// The networks allowed to connect.
    #[confval(default)]
    pub allow: Vec<Located<String>>,
    /// How each limit breach is handled. A constrained list in the bare shape.
    #[confval(default, keywords = LimitMode)]
    pub modes: Vec<Located<String>>,
    /// The same set in the optional-wrapped shape, so both reach the editor.
    #[confval(keywords = LimitMode)]
    pub events: Option<Located<Vec<Located<String>>>>,
    /// Extra response headers, by name.
    #[confval(map, default)]
    pub headers: BTreeMap<String, Located<String>>,
    /// Request limits.
    #[confval(nested)]
    pub limits: Option<Located<LimitsSpec>>,
    /// Zero or more routing rules. A repeated block.
    #[confval(nested)]
    pub rules: Vec<Located<RuleSpec>>,
}

/// A repeated routing-rule block of the fixture.
#[derive(confval::Spec)]
pub struct RuleSpec {
    /// The path prefix this rule matches.
    pub prefix: Located<String>,
}

impl Validate for RuleSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// The nested limits block of the fixture.
#[derive(confval::Spec)]
#[confval(derive_default)]
pub struct LimitsSpec {
    /// The largest request body, in megabytes.
    #[confval(default = 16, range = MAX_BODY_MB)]
    pub max_body_mb: Located<i64>,
    /// How a limit breach is handled.
    #[confval(default = "enforce".to_string(), keywords = LimitMode)]
    pub mode: Located<String>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        if self.hostname.value.is_empty() {
            report
                .error("hostname must not be empty")
                .at(self.hostname.span)
                .help("Set hostname to a reachable address, e.g. \"127.0.0.1\".")
                .emit();
        }
    }
}

/// A mesh-shaped fixture for the scoped reference tests: the labeled
/// `upstreams` block is nested inside the repeated `services` block, so a
/// route's reference resolves against its own service's upstreams rather than
/// a root-level block.
#[derive(confval::Spec)]
pub struct MeshSpec {
    /// The services, each with its own upstreams and routes.
    #[confval(nested)]
    pub services: Vec<Located<MeshServiceSpec>>,
    /// Root-level pools, shadowed by a service's own pools.
    #[confval(nested)]
    pub pools: Vec<Located<MeshPoolSpec>>,
}

/// A labeled pool block, declared at the root and inside a service, so the
/// shadowing rule has a fixture.
#[derive(confval::Spec)]
pub struct MeshPoolSpec {
    /// The pool's label.
    #[confval(label)]
    pub id: Located<String>,
}

impl Validate for MeshPoolSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// One service of the mesh fixture.
#[derive(confval::Spec)]
pub struct MeshServiceSpec {
    /// The service name.
    pub name: Located<String>,
    /// The service's upstreams. A repeated, labeled block.
    #[confval(nested)]
    pub upstreams: Vec<Located<MeshUpstreamSpec>>,
    /// The service's routes, each naming one of its upstreams.
    #[confval(nested)]
    pub routes: Vec<Located<MeshRouteSpec>>,
    /// The service's own pools, which shadow the root-level pools.
    #[confval(nested)]
    pub pools: Vec<Located<MeshPoolSpec>>,
}

/// A labeled upstream of one mesh service.
#[derive(confval::Spec)]
pub struct MeshUpstreamSpec {
    /// The upstream's label.
    #[confval(label)]
    pub name: Located<String>,
    /// The upstream port.
    pub port: Located<i64>,
}

/// A route of one mesh service.
#[derive(confval::Spec)]
pub struct MeshRouteSpec {
    /// The upstream this route targets, within its own service.
    #[confval(references = upstreams)]
    pub upstream: Located<String>,
    /// The pool this route uses, resolving at the nearest declaring scope.
    #[confval(references = pools)]
    pub pool: Option<Located<String>>,
}

impl Validate for MeshSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MeshServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MeshUpstreamSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for MeshRouteSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// A parent-and-child fixture whose block repeats a parent field name, for the
/// pending-body tests. The shared `port` name makes a wrong resolution level
/// visible, because a pending `admin` body must not read the root's `port` as
/// set.
#[derive(confval::Spec)]
pub struct RelaySpec {
    /// The TCP port the relay listens on.
    pub port: Located<i64>,
    /// The admin endpoint.
    #[confval(nested)]
    pub admin: Option<Located<AdminSpec>>,
}

/// The nested admin block of the relay fixture.
#[derive(confval::Spec)]
pub struct AdminSpec {
    /// The TCP port the admin endpoint listens on.
    pub port: Located<i64>,
}

impl Validate for RelaySpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for AdminSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// A Gateway-shaped fixture for the label and reference tests.
///
/// It is kept separate from `ServerSpec`, so its label and reference fields do
/// not churn the other handler tests, and its types carry distinct names so they
/// do not collide with the `RuleSpec` above. The `upstream` block is labeled and
/// repeated, and a route references an upstream by its label.
#[derive(confval::Spec)]
pub struct GatewaySpec {
    /// The upstream services a route can name. A repeated, labeled block.
    #[confval(nested)]
    pub upstream: Vec<Located<Upstream>>,
    /// The routing rules, each naming an upstream.
    #[confval(nested)]
    pub routes: Vec<Located<Route>>,
}

/// A labeled, repeated upstream block of the Gateway fixture.
#[derive(confval::Spec)]
pub struct Upstream {
    /// The upstream's label.
    #[confval(label)]
    pub name: Located<String>,
    /// The upstream host.
    pub host: Located<String>,
    /// The upstream port.
    pub port: Located<i64>,
}

impl Validate for Upstream {
    fn validate(&self, _report: &mut Report) {}
}

/// A routing rule of the Gateway fixture, naming an upstream by its label.
#[derive(confval::Spec)]
pub struct Route {
    /// The path prefix this rule matches.
    pub prefix: Located<String>,
    /// The upstream this rule routes to.
    #[confval(references = upstream)]
    pub upstream: Located<String>,
}

impl Validate for Route {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for GatewaySpec {
    fn validate(&self, _report: &mut Report) {}
}
