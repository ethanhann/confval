//! The derived children of the handwritten root.
//!
//! These are ordinary `#[derive(Spec)]` types. They are here to be called by a
//! handwritten parent. `RouteSpec` calls a handwritten child through its `tls`
//! field.

use crate::tls::TlsSpec;
use confval::prelude::*;

range_constraint!(MAX_BODY_MB, i64, min: 1, max: 512);

#[derive(confval::Spec, Debug)]
#[confval(derive_default)]
pub struct LimitsSpec {
    /// The largest request body accepted, in megabytes.
    #[confval(default = 16)]
    pub max_body_mb: Located<i64>,
    /// How long a request may run before it is cut off.
    #[confval(default = 30)]
    pub timeout_secs: Located<i64>,
}

impl Validate for LimitsSpec {
    fn validate(&self, report: &mut Report) {
        MAX_BODY_MB.check_located(&self.max_body_mb, "max_body_mb", report);
    }
}

#[derive(confval::Spec, Debug)]
pub struct TelemetrySpec {
    /// Where traces are shipped.
    pub endpoint: Located<String>,
}

impl Validate for TelemetrySpec {
    fn validate(&self, _report: &mut Report) {}
}

/// A derived type holding a handwritten one. The generated parser calls
/// `TlsSpec::from_fields` and the generated write walks call its two walks.
/// Neither distinguishes it from a derived type.
#[derive(confval::Spec, Debug)]
pub struct RouteSpec {
    /// The path prefix this route matches.
    pub path: Located<String>,
    /// Where matching requests are sent.
    pub upstream: Located<String>,
    #[confval(nested)]
    pub tls: Located<TlsSpec>,
}

impl Validate for RouteSpec {
    fn validate(&self, report: &mut Report) {
        if !self.path.value.starts_with('/') {
            report
                .error(format!("path must start with '/': {}", self.path.value))
                .at(self.path.span)
                .help("Write the prefix as a rooted path, e.g. \"/api\".")
                .emit();
        }
    }
}
