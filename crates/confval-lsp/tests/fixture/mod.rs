//! The fixture spec the handlers are tested against.
//!
//! It mirrors the `common` example spec, the representative shape the schema IR
//! is pinned against: a required scalar, a defaulted scalar with a range, a
//! keyword field, a string list, a map, and an optional nested block.
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
