//! What `validate_all` walks, and what a `descend` override stops.
//!
//! The other examples run one config through the whole pipeline. This one runs
//! the same invalid config twice, changing only whether the enclosing block is
//! enabled, and prints both reports.
//!
//! It renders its own reports rather than reusing `common::validate_and_gate`.
//! That helper is the pipeline gate and exits the process on the first error.
//! This example has to survive the first report to print the second.
//!
//! Run with: cargo run -p confval --example validate_traversal --features derive,color,toml

use confval::prelude::*;

range_constraint!(ATTEMPTS, i64, min: 1, max: 10);

#[derive(Default, confval::Spec)]
struct ServiceSpec {
    #[confval(nested)]
    upstream: Located<UpstreamSpec>,
}

/// A container with nothing of its own to check. The empty `validate` still
/// gets the generated traversal. `upstream` is reached either way.
impl Validate for ServiceSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(Default, confval::Spec)]
struct UpstreamSpec {
    enable: Located<bool>,
    #[confval(nested)]
    retry: Located<RetrySpec>,
}

impl Validate for UpstreamSpec {
    fn validate(&self, _report: &mut Report) {}

    fn descend(&self) -> ControlFlow<()> {
        if self.enable.value {
            ControlFlow::Continue(())
        } else {
            // The block is off, so its children describe nothing that runs.
            ControlFlow::Break(())
        }
    }
}

#[derive(Default, confval::Spec)]
struct RetrySpec {
    attempts: Located<i64>,
}

impl Validate for RetrySpec {
    fn validate(&self, report: &mut Report) {
        ATTEMPTS.check_located(&self.attempts, "attempts", report);
    }
}

/// Parses one config, validates it, and prints whatever the report collected.
fn run(label: &str, input: &str) -> Result<(), String> {
    println!("{label}");

    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("service.toml", input);

    let spec: Option<ServiceSpec> = confval::format::toml::parse_toml(&sources, id, &mut report);
    let spec = spec.ok_or("parse returned None without reporting an error")?;

    // The only validation call in this example. Everything below the root in
    // the spec hierarchy is reached by the generated traversal.
    spec.validate_all(&mut report);

    if report.has_issues() {
        let mut out = String::new();
        report
            .render_pretty(&sources, &mut out)
            .map_err(|err| format!("could not render report: {err}"))?;
        print!("{out}");
    } else {
        println!("no issues\n");
    }
    Ok(())
}

fn main() -> Result<(), String> {
    // `attempts` is out of range in both configs. Only `enable` differs.
    run(
        "upstream enabled: the nested child is validated",
        r#"[upstream]
enable = true

[upstream.retry]
attempts = 99
"#,
    )?;

    run(
        "upstream disabled: descend breaks, so the child is skipped",
        r#"[upstream]
enable = false

[upstream.retry]
attempts = 99
"#,
    )
}
