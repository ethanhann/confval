//! A field can stay required in a document and still supply a value for the
//! generated `Default` impl, with `#[confval(derive_default = expr)]`.

use confval::format::{FieldsBuilder, FromFields, Walk};
use confval::prelude::{Located, Report, Validate};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(confval::Spec)]
#[confval(derive_default)]
struct Limits {
    #[confval(derive_default = 7)]
    size: Located<i64>,
}

impl Validate for Limits {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(derive_default = false)]
    enable: Located<bool>,
    #[confval(derive_default = 1)]
    version: Located<i64>,
    #[confval(derive_default = "open".to_string())]
    mode: Located<String>,
    #[confval(derive_default = PathBuf::from("default"))]
    path: Located<PathBuf>,
    #[confval(default)]
    optional_list: Vec<Located<String>>,
    #[confval(derive_default = vec![Located::detached("a".to_string())])]
    required_list: Vec<Located<String>>,
    #[confval(
        map,
        derive_default = BTreeMap::from([(
            "k".to_string(),
            Located::detached("v".to_string()),
        )])
    )]
    config: BTreeMap<String, Located<String>>,
    #[confval(nested, derive_default = Limits::default())]
    limits: Located<Limits>,
    #[confval(nested)]
    optional_limits: Option<Located<Limits>>,
    #[confval(nested, derive_default = vec![Located::detached(Limits::default())])]
    services: Vec<Located<Limits>>,
}

impl Validate for Spec {
    fn validate(&self, _report: &mut Report) {}
}

// A block label stays required in the document. The generated `Default` impl
// supplies a non-empty value for a spec built in Rust.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Labeled {
    #[confval(label, derive_default = "default-label".to_string())]
    name: Located<String>,
}

impl Validate for Labeled {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {
    let spec = Spec::default();
    assert!(!spec.enable.value);
    assert_eq!(spec.version.value, 1);
    assert_eq!(spec.mode.value, "open");
    assert_eq!(spec.path.value, PathBuf::from("default"));
    assert!(spec.optional_list.is_empty());
    assert_eq!(spec.required_list.len(), 1);
    assert_eq!(spec.required_list[0].value, "a");
    assert_eq!(spec.config.get("k").map(|value| value.value.as_str()), Some("v"));
    assert_eq!(spec.limits.value.size.value, 7);
    assert!(spec.optional_limits.is_none());
    assert_eq!(spec.services.len(), 1);
    assert_eq!(spec.services[0].value.size.value, 7);

    let labeled = Labeled::default();
    assert_eq!(labeled.name.value, "default-label");

    // The fields stay required, so an empty block reports each one.
    let fields = FieldsBuilder::new(Walk::Populated).finish();
    let mut report = Report::new();
    let parsed = Spec::from_fields(&fields, &mut report);
    assert!(parsed.is_none(), "a required field must still be reported");
    let missing = report
        .issues()
        .iter()
        .filter(|issue| issue.message.starts_with("missing required field:"))
        .count();
    assert_eq!(missing, 7, "issues: {:?}", report.issues());

    let mut label_report = Report::new();
    let labeled = Labeled::from_fields(&fields, &mut label_report);
    assert!(labeled.is_none(), "a label must still be set");
    assert!(
        label_report.issues().iter().any(|issue| {
            issue.message.starts_with("missing required field:")
                && issue.message.contains("name")
        }),
        "issues: {:?}",
        label_report.issues()
    );
}
