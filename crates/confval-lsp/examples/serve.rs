//! A runnable language server over stdio, for trying the core against an editor.
//!
//! Run it with `cargo run -p confval-lsp --example serve [hcl|toml|kdl]`, then
//! point an LSP client at the built binary. It binds the core to the demo spec
//! below, so it is a testing convenience rather than a real deployment. The real
//! server names its own root spec and lives in the snakeway repository.
//!
//! The demo spec mirrors the crate's test fixture, so the sample documents under
//! `data/tmp/` parse against it.

use std::collections::BTreeMap;

use confval::prelude::*;

use confval_lsp::{Hcl, Kdl, Toml, serve};

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
    /// Zero or more routing rules.
    #[confval(nested)]
    rules: Vec<Located<RuleSpec>>,
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
}

impl Validate for LimitsSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for RuleSpec {
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
        _ => serve::<ServerSpec, Hcl>(Hcl),
    }
}
