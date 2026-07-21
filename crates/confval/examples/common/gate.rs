use confval::prelude::*;

pub(crate) fn evaluate_report(sources: &SourceMap, report: &Report) {
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
