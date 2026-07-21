use confval::prelude::*;

pub(crate) fn evaluate_report(sources: &SourceMap, report: &Report) {
    if report.has_issues() {
        let mut out = String::new();
        report.render_pretty(&sources, &mut out).unwrap();
        eprint!("{out}");
    }

    if report.has_errors() {
        std::process::exit(1);
    }
}
