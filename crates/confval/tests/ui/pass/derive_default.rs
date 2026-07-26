//! `#[confval(derive_default)]` pass test.
//!
//! Covers every field shape the mapping supports and asserts each field of
//! `Default::default()` against the value the parser fills for an absent field.
//! A nested type that also derives its default proves the feature composes.

use confval::prelude::{Located, Report, Validate};
use std::path::PathBuf;

#[derive(confval::Spec)]
#[confval(derive_default)]
struct Inner {
    #[confval(default = 7)]
    n: Located<i64>,
}

impl Validate for Inner {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct All {
    #[confval(default = 16)]
    expr_leaf: Located<i64>,
    #[confval(default)]
    bare_leaf: Located<i64>,
    #[confval(default)]
    bare_path: Located<PathBuf>,
    #[confval(default = "x".to_string())]
    opt_leaf_default: Option<Located<String>>,
    opt_leaf: Option<Located<i64>>,
    #[confval(default)]
    list: Vec<Located<String>>,
    opt_list: Option<Located<Vec<Located<String>>>>,
    #[confval(nested, default)]
    nested: Located<Inner>,
    #[confval(nested)]
    opt_nested: Option<Located<Inner>>,
    #[confval(nested)]
    nested_list: Vec<Located<Inner>>,
}

impl Validate for All {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {
    let d = All::default();
    assert_eq!(d.expr_leaf.value, 16);
    assert_eq!(d.bare_leaf.value, 0);
    assert_eq!(d.bare_path.value, PathBuf::new());
    assert_eq!(
        d.opt_leaf_default.as_ref().map(|v| v.value.as_str()),
        Some("x")
    );
    assert!(d.opt_leaf.is_none());
    assert!(d.list.is_empty());
    assert!(d.opt_list.is_none());
    assert_eq!(d.nested.value.n.value, 7);
    assert!(d.opt_nested.is_none());
    assert!(d.nested_list.is_empty());
}
