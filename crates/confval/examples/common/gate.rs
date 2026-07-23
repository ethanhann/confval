use confval::prelude::*;

/// Validates a spec, renders whatever the report collected, and stops the
/// program if an error was found.
///
/// In a real system with hot reload, `std::process::exit(1)` would not be
/// called. Instead, the `report.has_errors()` gate would tell the system
/// to not do the hot reload, and the reload request would be gracefully
/// stopped.
pub(crate) fn validate_and_gate<S>(spec: &S, sources: &SourceMap, report: &mut Report)
where
    S: Validate + ValidateNested,
{
    spec.validate_all(report);

    if report.has_issues() {
        let mut out = String::new();
        let result = report.render_pretty(sources, &mut out);
        match result {
            Ok(_) => eprint!("{out}"),
            Err(err) => eprint!("Could not render report. {}", err),
        }
    }

    if report.has_errors() {
        std::process::exit(1);
    }
}
