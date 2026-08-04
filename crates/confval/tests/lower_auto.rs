//! Guards the auto-mapped lowering of container fields.
//!
//! `#[derive(Config)]` lowers a field whose spec and config types share a name
//! and inner type through `LowerAuto`, which unwraps the `Located` layers
//! without a converter. Four of the five impls carry a container: an optional
//! scalar, a bare list, a wrapped list, and an optional wrapped list.
//!
//! Asserting only that lowering succeeded would pass for an impl that dropped
//! every element, so each test compares the lowered value against the input it
//! was built from.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "derive")]

use confval::prelude::*;

#[derive(confval::Spec)]
struct ContainerSpec {
    pid_file: Option<Located<String>>,
    allow: Vec<Located<String>>,
    tags: Option<Located<Vec<Located<String>>>>,
}

impl Validate for ContainerSpec {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Config)]
#[confval(lower_from = ContainerSpec)]
struct ContainerConfig {
    pid_file: Option<String>,
    allow: Vec<String>,
    tags: Option<Vec<String>>,
}

/// A spec whose every container is populated, with distinct values so a lowered
/// field cannot pass by matching a sibling.
fn populated() -> ContainerSpec {
    ContainerSpec {
        pid_file: Some(Located::detached("/var/run/app.pid".to_string())),
        allow: vec![
            Located::detached("10.0.0.0/8".to_string()),
            Located::detached("192.168.0.0/16".to_string()),
        ],
        tags: Some(Located::detached(vec![
            Located::detached("edge".to_string()),
            Located::detached("beta".to_string()),
        ])),
    }
}

fn lower(spec: &ContainerSpec) -> ContainerConfig {
    let mut report = Report::new();
    let config = ContainerConfig::lower(spec, &mut report);
    assert!(!report.has_issues(), "issues: {:?}", report.issues());
    config.expect("a populated spec should lower")
}

#[test]
fn an_optional_scalar_keeps_its_value() {
    // Arrange
    let spec = populated();

    // Act
    let config = lower(&spec);

    // Assert
    assert_eq!(config.pid_file.as_deref(), Some("/var/run/app.pid"));
}

#[test]
fn a_bare_list_keeps_every_element_in_order() {
    // Arrange
    let spec = populated();

    // Act
    let config = lower(&spec);

    // Assert
    assert_eq!(config.allow, vec!["10.0.0.0/8", "192.168.0.0/16"]);
}

#[test]
fn an_optional_wrapped_list_keeps_every_element_in_order() {
    // Arrange
    let spec = populated();

    // Act
    let config = lower(&spec);

    // Assert
    assert_eq!(
        config.tags,
        Some(vec!["edge".to_string(), "beta".to_string()])
    );
}

#[test]
fn an_absent_optional_scalar_lowers_to_none() {
    // Arrange
    let mut spec = populated();
    spec.pid_file = None;

    // Act
    let config = lower(&spec);

    // Assert
    // The absent case and the populated case must not collapse into one
    // another, which is what an impl returning a constant would do.
    assert_eq!(config.pid_file, None);
    assert_eq!(config.allow.len(), 2);
}

#[test]
fn an_absent_optional_wrapped_list_lowers_to_none() {
    // Arrange
    let mut spec = populated();
    spec.tags = None;

    // Act
    let config = lower(&spec);

    // Assert
    assert_eq!(config.tags, None);
}

#[test]
fn an_empty_bare_list_lowers_to_an_empty_vec() {
    // Arrange
    let mut spec = populated();
    spec.allow = Vec::new();

    // Act
    let config = lower(&spec);

    // Assert
    assert!(config.allow.is_empty());
    assert_eq!(config.pid_file.as_deref(), Some("/var/run/app.pid"));
}

#[test]
fn a_source_written_empty_wrapped_list_stays_some_and_empty() {
    // Arrange
    // `Some(empty)` and `None` are different answers, so an impl that folded
    // one into the other would pass a test that only checked the element count.
    let mut spec = populated();
    spec.tags = Some(Located::detached(Vec::new()));

    // Act
    let config = lower(&spec);

    // Assert
    assert_eq!(config.tags, Some(Vec::new()));
}
