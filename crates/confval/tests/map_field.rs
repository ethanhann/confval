//! The map field, `#[confval(map)]`: round trip across every frontend, the
//! parser's required and default behavior, the populate and source-view walks,
//! and the runtime lowering.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "derive")]

use confval::format::{FieldKind, ToFields, Value, ValueKind};
use confval::prelude::*;
use std::collections::{BTreeMap, HashMap};

/// A map with a bare default, so an absent map reads as empty.
#[derive(confval::Spec)]
struct Cfg {
    #[confval(map, default)]
    headers: BTreeMap<String, Located<String>>,
}

impl Validate for Cfg {
    fn validate(&self, _report: &mut Report) {}
}

/// The same map without a default, so an absent map is a missing-field error.
#[derive(confval::Spec)]
struct RequiredCfg {
    #[confval(map)]
    headers: BTreeMap<String, Located<String>>,
}

impl Validate for RequiredCfg {
    fn validate(&self, _report: &mut Report) {}
}

/// A runtime type that auto-lowers the map to a plain `HashMap`.
#[derive(confval::Config)]
#[confval(lower_from = Cfg)]
struct CfgRuntime {
    headers: HashMap<String, String>,
}

/// A runtime type that auto-lowers the map to a sorted `BTreeMap`.
#[derive(confval::Config)]
#[confval(lower_from = Cfg)]
struct SortedRuntime {
    headers: BTreeMap<String, String>,
}

/// A `Cfg` built by hand, its map values detached, the shape `to_fields`
/// produces.
fn cfg_with(entries: &[(&str, &str)]) -> Cfg {
    Cfg {
        headers: entries
            .iter()
            .map(|(key, value)| (key.to_string(), Located::detached(value.to_string())))
            .collect(),
    }
}

/// A map's keys and values, spans dropped, for comparison.
fn plain(headers: &BTreeMap<String, Located<String>>) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(key, value)| (key.clone(), value.value.clone()))
        .collect()
}

fn expected(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// The two entries every round-trip test uses. One key is a valid identifier in
/// no format's bare form, `cache.max-age`, so the emit must quote it.
const ENTRIES: &[(&str, &str)] = &[
    ("Content-Type", "application/json"),
    ("cache.max-age", "60"),
];

// A round trip per frontend: parse, emit the populated spec, reparse, and
// assert the resolved map survives, keys and values, including the
// non-identifier key.

#[cfg(feature = "hcl")]
#[test]
fn hcl_round_trips_a_map_with_a_non_identifier_key() {
    let input = r#"headers = { "Content-Type" = "application/json", "cache.max-age" = "60" }"#;
    let parsed = parse::<Cfg>(input, "c.hcl", confval::format::hcl::parse_hcl);
    assert_eq!(plain(&parsed.headers), expected(ENTRIES));

    let emitted = confval::format::hcl::emit_hcl(&parsed.to_fields()).expect("emit");
    let reparsed = parse::<Cfg>(&emitted, "r.hcl", confval::format::hcl::parse_hcl);
    assert_eq!(
        plain(&reparsed.headers),
        expected(ENTRIES),
        "emitted:\n{emitted}"
    );
}

#[cfg(feature = "toml")]
#[test]
fn toml_round_trips_a_map_with_a_non_identifier_key() {
    let input = r#"headers = { "Content-Type" = "application/json", "cache.max-age" = "60" }"#;
    let parsed = parse::<Cfg>(input, "c.toml", confval::format::toml::parse_toml);
    assert_eq!(plain(&parsed.headers), expected(ENTRIES));

    let emitted = confval::format::toml::emit_toml(&parsed.to_fields()).expect("emit");
    let reparsed = parse::<Cfg>(&emitted, "r.toml", confval::format::toml::parse_toml);
    assert_eq!(
        plain(&reparsed.headers),
        expected(ENTRIES),
        "emitted:\n{emitted}"
    );
}

#[cfg(feature = "kdl")]
#[test]
fn kdl_round_trips_a_map_with_a_non_identifier_key() {
    let input =
        "headers {\n    \"Content-Type\" \"application/json\"\n    \"cache.max-age\" \"60\"\n}\n";
    let parsed = parse::<Cfg>(input, "c.kdl", confval::format::kdl::parse_kdl);
    assert_eq!(plain(&parsed.headers), expected(ENTRIES));

    let emitted = confval::format::kdl::emit_kdl(&parsed.to_fields()).expect("emit");
    let reparsed = parse::<Cfg>(&emitted, "r.kdl", confval::format::kdl::parse_kdl);
    assert_eq!(
        plain(&reparsed.headers),
        expected(ENTRIES),
        "emitted:\n{emitted}"
    );
}

#[cfg(feature = "json")]
#[test]
fn json_round_trips_a_map_with_a_non_identifier_key() {
    let input = r#"{ "headers": { "Content-Type": "application/json", "cache.max-age": "60" } }"#;
    let parsed = parse::<Cfg>(input, "c.json", confval::format::json::parse_json);
    assert_eq!(plain(&parsed.headers), expected(ENTRIES));

    let emitted = confval::format::json::emit_json(&parsed.to_fields()).expect("emit");
    let reparsed = parse::<Cfg>(&emitted, "r.json", confval::format::json::parse_json);
    assert_eq!(
        plain(&reparsed.headers),
        expected(ENTRIES),
        "emitted:\n{emitted}"
    );
}

#[cfg(feature = "yaml")]
#[test]
fn yaml_round_trips_a_map_with_a_non_identifier_key() {
    let input = "headers:\n  \"Content-Type\": \"application/json\"\n  \"cache.max-age\": \"60\"\n";
    let parsed = parse::<Cfg>(input, "c.yaml", confval::format::yaml::parse_yaml);
    assert_eq!(plain(&parsed.headers), expected(ENTRIES));

    let emitted = confval::format::yaml::emit_yaml(&parsed.to_fields()).expect("emit");
    let reparsed = parse::<Cfg>(&emitted, "r.yaml", confval::format::yaml::parse_yaml);
    assert_eq!(
        plain(&reparsed.headers),
        expected(ENTRIES),
        "emitted:\n{emitted}"
    );
}

// Parser behavior, driven through TOML.

#[cfg(feature = "toml")]
#[test]
fn absent_map_reads_as_empty_under_default() {
    // Act
    let cfg = parse::<Cfg>("", "empty.toml", confval::format::toml::parse_toml);

    // Assert
    assert!(cfg.headers.is_empty());
}

#[cfg(feature = "toml")]
#[test]
fn a_present_empty_map_reads_as_empty_not_missing() {
    // Act
    let cfg = parse::<RequiredCfg>(
        "headers = {}",
        "empty-map.toml",
        confval::format::toml::parse_toml,
    );

    // Assert
    assert!(cfg.headers.is_empty());
}

#[cfg(feature = "toml")]
#[test]
fn a_required_map_reports_missing_when_absent() {
    // Arrange
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("missing.toml", "");

    // Act
    let cfg: Option<RequiredCfg> = confval::format::toml::parse_toml(&sources, id, &mut report);

    // Assert
    assert!(cfg.is_none());
    assert!(
        messages(&report)
            .iter()
            .any(|m| m == "missing required field: headers")
    );
}

#[cfg(feature = "toml")]
#[test]
fn a_present_but_invalid_map_is_not_also_reported_missing() {
    // Arrange
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add("bad.toml", "headers = { port = 8080 }");

    // Act
    let cfg: Option<RequiredCfg> = confval::format::toml::parse_toml(&sources, id, &mut report);

    // Assert
    assert!(cfg.is_none());
    let messages = messages(&report);
    assert!(
        messages
            .iter()
            .any(|m| m == "expected string, found number")
    );
    assert!(
        !messages
            .iter()
            .any(|m| m == "missing required field: headers")
    );
}

// The populate walk.

#[test]
fn populate_emits_a_map_value_in_sorted_key_order() {
    // Arrange
    let cfg = cfg_with(&[("b", "2"), ("a", "1")]);

    // Act
    let fields = cfg.to_fields();

    // Assert
    let keys = map_keys(&fields).expect("headers is a map value");
    assert_eq!(keys, vec!["a", "b"]);
}

#[test]
fn populate_emits_an_empty_map_value_for_an_empty_map() {
    // Arrange
    let cfg = cfg_with(&[]);

    // Act
    let fields = cfg.to_fields();

    // Assert
    assert_eq!(
        map_keys(&fields).expect("headers is a map value"),
        Vec::<String>::new()
    );
}

// The source-view walk.

#[cfg(feature = "toml")]
#[test]
fn source_view_emits_a_source_written_map() {
    // Arrange
    let cfg = parse::<Cfg>(
        r#"headers = { a = "1" }"#,
        "s.toml",
        confval::format::toml::parse_toml,
    );

    // Act
    let source = cfg.to_source_fields();

    // Assert
    assert_eq!(map_keys(&source).expect("headers present"), vec!["a"]);
}

#[test]
fn source_view_omits_a_detached_map() {
    // Arrange
    let cfg = cfg_with(&[("a", "1")]);

    // Act
    let source = cfg.to_source_fields();

    // Assert
    assert!(source.get("headers").is_none());
}

// The runtime lowering.

#[test]
fn lowers_to_a_hashmap_dropping_spans() {
    // Arrange
    let cfg = cfg_with(&[("a", "1"), ("b", "2")]);
    let mut report = Report::new();

    // Act
    let runtime = CfgRuntime::lower(&cfg, &mut report).expect("lower");

    // Assert
    let mut pairs: Vec<_> = runtime.headers.into_iter().collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string())
        ]
    );
}

#[test]
fn lowers_to_a_sorted_btreemap() {
    // Arrange
    let cfg = cfg_with(&[("b", "2"), ("a", "1")]);
    let mut report = Report::new();

    // Act
    let runtime = SortedRuntime::lower(&cfg, &mut report).expect("lower");

    // Assert
    let keys: Vec<_> = runtime.headers.keys().cloned().collect();
    assert_eq!(keys, vec!["a", "b"]);
}

// Helpers.

/// Parses `input` through a frontend and asserts a clean report.
fn parse<S: confval::format::FromFields>(
    input: &str,
    name: &str,
    frontend: fn(&SourceMap, confval::source::SourceId, &mut Report) -> Option<S>,
) -> S {
    let mut sources = SourceMap::new();
    let mut report = Report::new();
    let id = sources.add(name, input);
    let parsed = frontend(&sources, id, &mut report);
    assert!(!report.has_errors(), "parse errors in {name}");
    parsed.expect("parse returned a value")
}

/// The keys of the `headers` field when it is a map value, in source order, or
/// `None` when the field is absent or not a map value.
fn map_keys(fields: &confval::format::Fields) -> Option<Vec<String>> {
    let field = fields.get("headers")?;
    let FieldKind::Value(Value {
        kind: ValueKind::Map(inner),
        ..
    }) = &field.kind
    else {
        return None;
    };
    Some(inner.iter().map(|entry| entry.name.clone()).collect())
}

/// Every diagnostic message in the report.
#[cfg(feature = "toml")]
fn messages(report: &Report) -> Vec<String> {
    report
        .issues()
        .iter()
        .map(|issue| issue.message.clone())
        .collect()
}
