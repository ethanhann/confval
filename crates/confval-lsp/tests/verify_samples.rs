//! Throwaway: verifies the valid samples parse clean and the invalid ones fail.

mod fixture;

use confval::diagnostic::Report;
use confval::format::{Fields, FromFields};
use confval::prelude::*;
use confval::source::{SourceId, SourceMap};

use fixture::ServerSpec;

type ParseFn = fn(&SourceMap, SourceId, &mut Report) -> Option<Fields>;

fn issues(path: &str, parse: ParseFn) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut sources = SourceMap::new();
    let id = sources.add(path, &text);
    let mut report = Report::new();
    if let Some(fields) = parse(&sources, id, &mut report)
        && let Some(spec) = ServerSpec::from_fields(&fields, &mut report)
    {
        spec.validate_all(&mut report);
    }
    report.issues().iter().map(|i| i.message.clone()).collect()
}

#[test]
fn samples_behave() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dev/sample_configs");
    let cases: [(&str, ParseFn); 5] = [
        ("hcl", confval::format::hcl::parse_hcl_fields),
        ("toml", confval::format::toml::parse_toml_fields),
        ("kdl", confval::format::kdl::parse_kdl_fields),
        ("json", confval::format::json::parse_json_fields),
        ("yaml", confval::format::yaml::parse_yaml_fields),
    ];
    for (name, parse) in cases {
        let valid = issues(&format!("{dir}/valid.confval.{name}"), parse);
        let invalid = issues(&format!("{dir}/invalid.confval.{name}"), parse);
        eprintln!("{name} invalid issues: {invalid:?}");
        assert!(valid.is_empty(), "valid.{name}: {valid:?}");
        assert!(invalid.len() >= 5, "invalid.{name} should flag several: {invalid:?}");
    }
}
