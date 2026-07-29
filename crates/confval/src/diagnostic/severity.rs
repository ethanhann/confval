/// How serious an issue is. An error stops the pipeline at the next gate,
/// and a warning renders without stopping the run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Severity {
    /// A problem that must be fixed before the configuration is usable.
    #[default]
    Error,
    /// Something worth surfacing that does not block the run.
    Warning,
}
