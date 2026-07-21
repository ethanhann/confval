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
    // The spec field is `Option<Located<LimitsSpec>>`; with `default` an absent
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
    // No `narrow` helper covers a keyword. `narrow` handles integer widths and
    // durations, so a string to enum conversion is a `with` function of your
    // own.
    #[confval(lower(from = mode, with = mode_to_enum))]
    pub mode: Mode,
}

/// The runtime form of the `mode` keyword.
///
/// The spec holds a `Located<String>`, so a wrong value is reported by
/// `KeywordSet` alongside every other problem in the file.
/// The string narrows to this enum only after the gate, so nothing downstream
/// reparses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Enforce,
    Log,
    Off,
}

/// Mechanically lower the string to the mode.
/// Even though it is capable of producing an error, it should never actually produce one.
impl TryFrom<&str> for Mode {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "enforce" => Ok(Self::Enforce),
            "log" => Ok(Self::Log),
            "off" => Ok(Self::Off),
            _ => Err(()),
        }
    }
}

fn mode_to_enum(value: &Located<String>, report: &mut Report) -> Option<Mode> {
    match Mode::try_from(value.value.as_str()) {
        Ok(mode) => Some(mode),
        // Validation already checked this against LIMIT_MODES, so reaching here
        // means there is a mismatch between the keyword set and the enum.
        // A keyword was added to one, but not the other.
        Err(_) => {
            report
                .error(format!("unknown mode: {}", value.value))
                .at(value.span)
                .help("This is likely a bug that should have been caught during validation.")
                .emit();
            None
        }
    }
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

/// This is for demo purposes, to see what the values are in the examples' output.
impl Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.write_str(match self {
            Self::Enforce => "enforce",
            Self::Log => "log",
            Self::Off => "off",
        })
    }
}
