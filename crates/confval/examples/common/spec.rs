use confval::prelude::*;

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);

const LIMIT_MODES: [&str; 3] = ["enforce", "log", "off"];

#[derive(confval::Spec)]
pub struct ServerSpec {
    pub hostname: Located<String>,
    pub port: Located<i64>,
    #[confval(default = 4)]
    pub workers: Located<i64>,
    // Optional in the source: when the block is omitted the spec keeps it
    // `None`, so a spec dump stays source-faithful. The config side fills the
    // default at lowering time.
    #[confval(nested)]
    pub limits: Option<Located<LimitsSpec>>,
}

#[derive(confval::Spec)]
pub struct LimitsSpec {
    #[confval(default = 16)]
    pub max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string())]
    pub mode: Located<String>,
}

impl Default for LimitsSpec {
    fn default() -> Self {
        Self {
            max_body_mb: Located::detached(16),
            mode: Located::detached("enforce".to_string()),
        }
    }
}

impl Validate for LimitsSpec {
    fn validate(&self, report: &mut Report) {
        MAX_BODY_MB.check_located(&self.max_body_mb, "max_body_mb", report);
        KeywordSet::new(&LIMIT_MODES).check_located(&self.mode, "mode", report);
    }
}

impl Validate for ServerSpec {
    fn validate(&self, report: &mut Report) {
        PORT.check_located(&self.port, "port", report);
        WORKERS.check_located(&self.workers, "workers", report);

        if self.hostname.value.is_empty() {
            report
                .error("hostname must not be empty")
                .at(self.hostname.span)
                .help("Set hostname to a reachable address, e.g. \"127.0.0.1\".")
                .emit();
        }
    }
}

/// A `Validate` impl only checks its own fields, so walking into the nested
/// block is the caller's job.
pub fn validate_server(spec: &ServerSpec, report: &mut Report) {
    spec.validate(report);

    if let Some(limits) = &spec.limits {
        limits.value.validate(report);
    }
}
