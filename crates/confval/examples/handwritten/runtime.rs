//! The lowering half: the runtime config the program uses.
//!
//! `#[derive(Config)]` lowers the handwritten root without knowing it was
//! written by hand, and a derived route lowers its `tls` field through the
//! handwritten `Lower` impl at the bottom of this file. The one impl that has
//! to be handwritten is the enum's, for the same reason its parser is.

use crate::children::{LimitsSpec, RouteSpec, TelemetrySpec};
use crate::spec::{LogEvent, Phase, ServiceSpec};
use crate::tls::{TlsChallenge, TlsSpec};
use confval::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

// Dead code analysis does not count a `Debug` print as a read of a field, so
// each runtime type carries `#[allow(dead_code)]`.
#[allow(dead_code)]
#[derive(confval::Config, Debug)]
#[confval(lower_from = ServiceSpec)]
pub struct ServiceConfig {
    #[confval(lower(from = name, with = lower_string))]
    pub name: String,
    #[confval(lower(from = workers, with = narrow::i64_to_usize))]
    pub workers: usize,
    #[confval(lower(from = sample_rate, with = lower_f64))]
    pub sample_rate: f64,
    #[confval(lower(from = verbose, with = lower_bool))]
    pub verbose: bool,
    #[confval(lower(from = pid_file, with = lower_opt_path))]
    pub pid_file: Option<PathBuf>,
    // A keyword list lowers through one call that reports every bad element.
    #[confval(lower(from = events, with = narrow::keyword_list::<LogEvent>))]
    pub events: Vec<LogEvent>,
    #[confval(lower(from = phases, with = narrow::opt_keyword_list::<Phase>))]
    pub phases: Option<Vec<Phase>>,
    #[confval(nested)]
    pub limits: LimitsConfig,
    #[confval(nested)]
    pub telemetry: Option<TelemetryConfig>,
    // The config field matches the spec's Rust field name, `routes`, while the
    // config file's key is `route`. A handwritten parser chooses both, which a
    // derived spec cannot.
    #[confval(nested)]
    pub routes: Vec<RouteConfig>,
}

#[allow(dead_code)]
#[derive(confval::Config, Debug)]
#[confval(lower_from = LimitsSpec)]
pub struct LimitsConfig {
    #[confval(lower(from = max_body_mb, with = narrow::i64_to_u32))]
    pub max_body_mb: u32,
    #[confval(lower(from = timeout_secs, with = narrow::i64_secs_to_duration))]
    pub timeout: Duration,
}

#[allow(dead_code)]
#[derive(confval::Config, Debug)]
#[confval(lower_from = TelemetrySpec)]
pub struct TelemetryConfig {
    #[confval(lower(from = endpoint, with = lower_string))]
    pub endpoint: String,
}

/// A derived config whose `tls` field lowers through a handwritten `Lower`.
#[allow(dead_code)]
#[derive(confval::Config, Debug)]
#[confval(lower_from = RouteSpec)]
pub struct RouteConfig {
    #[confval(lower(from = path, with = lower_string))]
    pub path: String,
    #[confval(lower(from = upstream, with = lower_string))]
    pub upstream: String,
    #[confval(nested)]
    pub tls: TlsConfig,
}

/// The runtime form of the tagged enum. `#[derive(Config)]` cannot express a
/// variant any more than `#[derive(Spec)]` can, so the `Lower` impl below is
/// written by hand.
#[allow(dead_code)]
#[derive(Debug)]
pub enum TlsConfig {
    Manual {
        cert: PathBuf,
        key: PathBuf,
    },
    Acme {
        domains: Vec<String>,
        challenge: TlsChallenge,
    },
}

impl Lower<TlsSpec> for TlsConfig {
    fn lower(spec: &TlsSpec, report: &mut Report) -> Option<Self> {
        match spec {
            TlsSpec::Manual { cert, key } => Some(TlsConfig::Manual {
                cert: cert.value.clone(),
                key: key.value.clone(),
            }),
            TlsSpec::Acme { domains, challenge } => Some(TlsConfig::Acme {
                domains: domains.iter().map(|domain| domain.value.clone()).collect(),
                // The keyword was checked in `Validate`, so this conversion
                // reports only when that check was left out.
                challenge: narrow::keyword(challenge, report)?,
            }),
        }
    }
}

/// The `with` functions for fields that need no conversion. A `with` attribute
/// cannot hold a closure, so each plain clone is a named function.
fn lower_string(value: &Located<String>, _report: &mut Report) -> Option<String> {
    Some(value.value.clone())
}

fn lower_f64(value: &Located<f64>, _report: &mut Report) -> Option<f64> {
    Some(value.value)
}

fn lower_bool(value: &Located<bool>, _report: &mut Report) -> Option<bool> {
    Some(value.value)
}

fn lower_opt_path(
    value: &Option<Located<PathBuf>>,
    _report: &mut Report,
) -> Option<Option<PathBuf>> {
    Some(value.as_ref().map(|path| path.value.clone()))
}
