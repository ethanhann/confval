use super::spec::*;
use confval::prelude::*;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
pub struct ServerConfig {
    pub hostname: String,
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    pub port: u16,
    #[confval(lower(from = workers, with = workers_to_usize))]
    pub workers: usize,
    // Only the layering example reads this, to show a bool coerced from a flag.
    #[allow(dead_code)]
    pub tls: bool,
    // The spec field is `Option<Located<LimitsSpec>>`. With `default` an absent
    // block lowers `LimitsSpec::default()` instead of producing a missing-field
    // error, and the runtime field stays non-optional.
    #[confval(nested, default)]
    pub limits: LimitsConfig,
}

#[derive(confval::Config)]
#[confval(lower_from = LimitsSpec)]
pub struct LimitsConfig {
    #[confval(lower(from = max_body_mb, with = narrow::i64_to_u16))]
    pub max_body_mb: u16,
    // `narrow::keyword` lowers the validated string through the `TryFrom` that
    // `keyword_enum!` generates, so a keyword needs no handwritten converter.
    #[confval(lower(from = mode, with = narrow::keyword::<LimitMode>))]
    pub mode: LimitMode,
}

fn workers_to_usize(value: &Located<i64>, _report: &mut Report) -> Option<usize> {
    // Safe: the range was validated and lowering only runs on a clean report.
    Some(value.value as usize)
}

/// This is for demo purposes, to see what the values are in the examples' output.
impl Display for ServerConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(
            f,
            "listening on {}:{} with {} workers",
            self.hostname, self.port, self.workers
        )?;
        writeln!(
            f,
            "limits: max_body_mb={} mode={}",
            self.limits.max_body_mb, self.limits.mode
        )
    }
}
