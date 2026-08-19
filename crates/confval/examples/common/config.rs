use super::spec::*;
use confval::prelude::*;
use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(confval::Config)]
#[confval(lower_from = ServerSpec)]
pub struct ServerConfig {
    pub hostname: String,
    #[confval(lower(from = port, with = narrow::i64_to_u16))]
    pub port: u16,
    #[confval(lower(from = workers, with = narrow::i64_to_usize))]
    pub workers: usize,
    // Only the layering example reads this, to show a bool coerced from a flag.
    #[allow(dead_code)]
    pub tls: bool,
    #[confval(lower(from = allow, with = allow_to_vec))]
    pub allow: Vec<String>,
    // Auto-mapped from the spec's `BTreeMap<String, Located<String>>`. The
    // `LowerAuto` impl drops each value's span and hands back a plain runtime
    // map.
    pub headers: HashMap<String, String>,
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

fn allow_to_vec(value: &[Located<String>], _report: &mut Report) -> Option<Vec<String>> {
    Some(value.iter().map(|entry| entry.value.clone()).collect())
}

impl Display for ServerConfig {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(
            f,
            "listening on {}:{} with {} workers",
            self.hostname, self.port, self.workers
        )?;
        if !self.allow.is_empty() {
            writeln!(f, "allow: {}", self.allow.join(", "))?;
        }
        if !self.headers.is_empty() {
            let mut entries: Vec<_> = self
                .headers
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            entries.sort();
            writeln!(f, "headers: {}", entries.join(", "))?;
        }
        writeln!(
            f,
            "limits: max_body_mb={} mode={}",
            self.limits.max_body_mb, self.limits.mode
        )
    }
}
