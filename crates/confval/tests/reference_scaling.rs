//! The reference pass must stay linear in the size of the document.
//!
//! `check_references` runs on every parse, and `confval-lsp` runs it on every
//! keystroke through its diagnostics handler. A pass whose cost grows faster
//! than the file makes a large configuration unusable in an editor, so the
//! growth rate is a behavior worth pinning.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use confval::diagnostic::Report;
use confval::format::hcl::parse_hcl_fields;
use confval::pipeline::{Validate, check_references};
use confval::schema::ToSchema;
use confval::source::{Located, SourceMap};

#[derive(confval::Spec)]
struct UpstreamSpec {
    #[confval(label)]
    name: Located<String>,
    host: Located<String>,
}

#[derive(confval::Spec)]
struct RuleSpec {
    prefix: Located<String>,
    #[confval(references = upstream)]
    upstream: Located<String>,
}

#[derive(confval::Spec)]
struct GatewaySpec {
    #[confval(nested)]
    upstream: Vec<Located<UpstreamSpec>>,
    #[confval(nested)]
    rules: Vec<Located<RuleSpec>>,
}

impl Validate for UpstreamSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for RuleSpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for GatewaySpec {
    fn validate(&self, _report: &mut Report) {}
}

/// A gateway document with `blocks` upstreams and `blocks` rules, where each
/// rule references one upstream by label.
fn gateway_text(blocks: usize) -> String {
    let mut out = String::new();
    for i in 0..blocks {
        out.push_str(&format!("upstream \"u{i}\" {{\n  host = \"h{i}\"\n}}\n"));
    }
    for i in 0..blocks {
        out.push_str(&format!(
            "rules {{\n  prefix = \"/p{i}\"\n  upstream = \"u{i}\"\n}}\n"
        ));
    }
    out
}

/// The fastest of three reference passes over a document of `blocks` blocks.
/// The fastest run is the least noisy estimate of the real cost.
fn check_cost(blocks: usize) -> Duration {
    let text = gateway_text(blocks);
    let mut sources = SourceMap::new();
    let id = sources.add("gateway.hcl", text);
    let mut report = Report::new();
    let fields = parse_hcl_fields(&sources, id, &mut report).expect("the document parses");
    let schema = GatewaySpec::schema();
    assert!(!report.has_errors(), "the document is valid");

    (0..3)
        .map(|_| {
            let mut report = Report::new();
            let start = Instant::now();
            check_references(&fields, &schema, &mut report);
            let elapsed = start.elapsed();
            assert!(!report.has_errors(), "every reference resolves");
            elapsed
        })
        .min()
        .expect("three runs")
}

#[test]
fn the_reference_pass_stays_linear_in_the_block_count() {
    // Arrange
    let small = check_cost(100);
    let large = check_cost(400);

    // Act
    let growth = large.as_secs_f64() / small.as_secs_f64();

    // Assert
    assert!(
        growth < 8.0,
        "four times the blocks cost {growth:.1} times the work ({small:?} to {large:?}), \
         so the pass is not linear"
    );
}
