//! Guards the span fidelity the `confval::format::toml` adapter depends on.
//! The adapter converts a toml_edit range into a span and falls back to the
//! detached sentinel when the range is missing, and the source view reads those
//! spans to decide what the operator wrote. A block whose span came back
//! detached would drop out of the source view even though it was written, so
//! these pin the attached side for an explicit block header and for the
//! implicit super-table a dotted header creates.

use confval::format::toml::parse_toml;
use confval::prelude::*;

#[derive(confval::Spec, Debug)]
#[confval(derive_default)]
struct Server {
    #[confval(nested, default)]
    limits: Located<Limits>,
}

impl Validate for Server {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, Debug)]
#[confval(derive_default)]
struct Limits {
    #[confval(default = 16)]
    size: Located<i64>,
}

impl Validate for Limits {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, Debug)]
struct Root {
    #[confval(nested)]
    server: Located<Server>,
}

impl Validate for Root {
    fn validate(&self, _report: &mut Report) {}
}

fn parse(text: &str) -> Root {
    let mut sources = SourceMap::new();
    let id = sources.add("nested.toml", text.to_string());
    let mut report = Report::new();
    let root = parse_toml::<Root>(&sources, id, &mut report)
        .unwrap_or_else(|| panic!("fixture should parse: {:?}", report.issues()));
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    root
}

#[test]
fn an_explicit_block_header_carries_an_attached_span() {
    // Arrange
    let text = "[server]\n\n[server.limits]\nsize = 1\n";

    // Act
    let root = parse(text);

    // Assert
    assert!(!root.server.span.is_detached(), "explicit header span");
    assert!(
        !root.server.value.limits.span.is_detached(),
        "nested block span"
    );
}

#[test]
fn an_implicit_super_table_carries_an_attached_span() {
    // Arrange
    // `[server.limits]` names `server` only as an implicit super-table, with no
    // explicit `[server]` header of its own.
    let text = "[server.limits]\nsize = 1\n";

    // Act
    let root = parse(text);

    // Assert
    assert!(!root.server.span.is_detached(), "implicit super-table span");
    assert!(
        !root.server.value.limits.span.is_detached(),
        "nested block span"
    );
}
