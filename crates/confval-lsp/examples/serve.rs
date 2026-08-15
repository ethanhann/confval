//! A runnable language server over stdio, for trying the core against an editor.
//!
//! Run it with `cargo run -p confval-lsp --example serve [hcl|toml|kdl|json|yaml]`, then
//! point an LSP client at the built binary. It binds the core to the demo spec
//! below, so it is a testing convenience rather than a real deployment. The real
//! server names its own root spec and lives in the snakeway repository.
//!
//! The demo spec mirrors the crate's test fixture, so the sample documents in
//! `dev/sample_configs/` parse against it.

use std::collections::BTreeMap;

use confval::prelude::*;

use confval_lsp::{Hcl, Json, Kdl, Toml, Yaml, serve};

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);

keyword_enum!(LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

/// The demo root spec.
#[derive(confval::Spec)]
struct ServerSpec {
    /// The address the server binds.
    hostname: Located<String>,
    /// The TCP port the server listens on.
    #[confval(range = PORT)]
    port: Located<i64>,
    /// The number of worker threads.
    #[confval(default = 4, range = WORKERS)]
    workers: Located<i64>,
    /// Whether to serve TLS.
    #[confval(default = false)]
    tls: Located<bool>,
    /// The networks allowed to connect.
    #[confval(default)]
    allow: Vec<Located<String>>,
    /// Extra response headers, by name.
    #[confval(map, default)]
    headers: BTreeMap<String, Located<String>>,
    /// Request limits.
    #[confval(nested)]
    limits: Option<Located<LimitsSpec>>,
    /// The upstream services a rule can route to. A repeated, labeled block.
    #[confval(nested)]
    upstream: Vec<Located<UpstreamSpec>>,
    /// Zero or more routing rules.
    #[confval(nested)]
    rules: Vec<Located<RuleSpec>>,
    /// Zero or more services, the sibling-scoped reference shape: a service's
    /// route names one of that service's own endpoints.
    #[confval(nested)]
    service: Vec<Located<ServiceSpec>>,
}

/// A repeated service block with its own endpoint pool.
#[derive(confval::Spec)]
struct ServiceSpec {
    /// The service name.
    name: Located<String>,
    /// The service's endpoints. A repeated, labeled block nested below the
    /// root, so a reference to it resolves within this service.
    #[confval(nested)]
    endpoints: Vec<Located<EndpointSpec>>,
    /// The service's routes, each naming one of its own endpoints.
    #[confval(nested)]
    routes: Vec<Located<ServiceRouteSpec>>,
}

/// A labeled endpoint of one service.
#[derive(confval::Spec)]
struct EndpointSpec {
    /// The endpoint's label, named by a service route's `endpoint` field.
    #[confval(label)]
    name: Located<String>,
    /// The endpoint port.
    #[confval(range = PORT)]
    port: Located<i64>,
}

/// A route of one service, resolving against that service's endpoints.
#[derive(confval::Spec)]
struct ServiceRouteSpec {
    /// The path prefix this route matches.
    prefix: Located<String>,
    /// The endpoint this route targets, within its own service.
    #[confval(references = endpoints)]
    endpoint: Option<Located<String>>,
}

/// A labeled, repeated upstream block.
#[derive(confval::Spec)]
struct UpstreamSpec {
    /// The upstream's label, named by a rule's `upstream` field.
    #[confval(label)]
    name: Located<String>,
    /// The upstream host.
    host: Located<String>,
    /// The upstream port.
    #[confval(range = PORT)]
    port: Located<i64>,
}

/// The nested limits block.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct LimitsSpec {
    /// The largest request body, in megabytes.
    #[confval(default = 16, range = MAX_BODY_MB)]
    max_body_mb: Located<i64>,
    /// How a limit breach is handled.
    #[confval(default = "enforce".to_string(), keywords = LimitMode)]
    mode: Located<String>,
}

/// A repeated routing-rule block.
#[derive(confval::Spec)]
struct RuleSpec {
    /// The path prefix this rule matches.
    prefix: Located<String>,
    /// The upstream this rule routes to, naming an `upstream` block by its label.
    #[confval(references = upstream)]
    upstream: Option<Located<String>>,
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for UpstreamSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for RuleSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for EndpointSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ServiceRouteSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        if self.hostname.value.is_empty() {
            report
                .error("hostname must not be empty")
                .at(self.hostname.span)
                .emit();
        }
        if self.hostname.value == "0.0.0.0" {
            report
                .warning("hostname listens on every network device")
                .at(self.hostname.span)
                .emit();
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match std::env::args().nth(1).as_deref() {
        Some("toml") => serve::<ServerSpec, Toml>(Toml),
        Some("kdl") => serve::<ServerSpec, Kdl>(Kdl),
        Some("json") => serve::<ServerSpec, Json>(Json),
        Some("yaml") => serve::<ServerSpec, Yaml>(Yaml),
        _ => serve::<ServerSpec, Hcl>(Hcl),
    }
}
