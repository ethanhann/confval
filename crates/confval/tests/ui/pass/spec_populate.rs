//! `ToFields` populate pass test.
//!
//! Covers every field shape, the per-leaf inverse into `Scalar`, the nested-list
//! block spelling, the populate marker on an optional nested block, and that
//! every span the walk produces is detached. A second assertion pins the
//! fixed-point property: populate adds defaults, so the first reparse of a
//! populated spec and the second agree.

use confval::format::{FieldKind, Fields, FromFields, Scalar, ToFields, Value, ValueKind};
use confval::prelude::{Located, Report, Validate};
use std::path::PathBuf;

#[derive(confval::Spec, PartialEq, Debug)]
#[confval(derive_default)]
struct Inner {
    #[confval(default = "inner".to_string())]
    name: Located<String>,
}

impl Validate for Inner {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct All {
    text: Located<String>,
    count: Located<i64>,
    ratio: Located<f64>,
    flag: Located<bool>,
    path: Located<PathBuf>,
    #[confval(default = 9)]
    defaulted: Located<i64>,
    opt_present: Option<Located<String>>,
    opt_absent: Option<Located<i64>>,
    list: Vec<Located<String>>,
    #[confval(nested)]
    req_block: Located<Inner>,
    #[confval(nested, default)]
    marked_absent: Option<Located<Inner>>,
    #[confval(nested)]
    unmarked_absent: Option<Located<Inner>>,
    #[confval(nested)]
    block_list: Vec<Located<Inner>>,
}

impl Validate for All {
    fn validate(&self, _report: &mut Report) {}
}

// A three-level chain of marked blocks, so a filled block that itself carries a
// marked absent block is exercised. This guards the recursion the milestone is
// built on, not just a one-level fill.
#[derive(confval::Spec, PartialEq, Debug)]
#[confval(derive_default)]
struct DeepLeaf {
    #[confval(default = 5)]
    n: Located<i64>,
}

impl Validate for DeepLeaf {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
#[confval(derive_default)]
struct DeepMid {
    #[confval(nested, default)]
    leaf: Option<Located<DeepLeaf>>,
}

impl Validate for DeepMid {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec, PartialEq, Debug)]
struct DeepTop {
    #[confval(nested, default)]
    mid: Option<Located<DeepMid>>,
}

impl Validate for DeepTop {
    fn validate(&self, _report: &mut Report) {}
}

// A zero-field spec, so the `to_fields` branch that drops `mut` for an empty
// item vector is compiled.
#[derive(confval::Spec, PartialEq, Debug)]
struct NoFields {}

impl Validate for NoFields {
    fn validate(&self, _report: &mut Report) {}
}

fn sample() -> All {
    All {
        text: Located::detached("api".to_string()),
        count: Located::detached(42),
        ratio: Located::detached(0.5),
        flag: Located::detached(true),
        path: Located::detached(PathBuf::from("/etc/app")),
        defaulted: Located::detached(9),
        opt_present: Some(Located::detached("here".to_string())),
        opt_absent: None,
        list: vec![
            Located::detached("a".to_string()),
            Located::detached("b".to_string()),
        ],
        req_block: Located::detached(Inner {
            name: Located::detached("root".to_string()),
        }),
        marked_absent: None,
        unmarked_absent: None,
        block_list: vec![
            Located::detached(Inner {
                name: Located::detached("x".to_string()),
            }),
            Located::detached(Inner {
                name: Located::detached("y".to_string()),
            }),
        ],
    }
}

fn scalar_of<'a>(fields: &'a Fields, name: &str) -> &'a Scalar {
    match &fields.get(name).expect("field present").kind {
        FieldKind::Value(Value {
            kind: ValueKind::Scalar(scalar),
            ..
        }) => scalar,
        _ => panic!("{name} is not a scalar attribute"),
    }
}

fn block_of<'a>(fields: &'a Fields, name: &str) -> &'a Fields {
    match &fields.get(name).expect("block present").kind {
        FieldKind::Block(inner) => inner,
        _ => panic!("{name} is not a block"),
    }
}

fn assert_all_detached(fields: &Fields) {
    assert!(fields.enclosing().is_detached());
    for field in fields.iter() {
        assert!(field.name_span.is_detached());
        assert!(field.span.is_detached());
        match &field.kind {
            FieldKind::Block(inner) => assert_all_detached(inner),
            FieldKind::Value(value) => assert_value_detached(value),
        }
    }
}

fn assert_value_detached(value: &Value) {
    assert!(value.span.is_detached());
    match &value.kind {
        ValueKind::Seq(elements) => elements.iter().for_each(assert_value_detached),
        ValueKind::Map(inner) => assert_all_detached(inner),
        _ => {}
    }
}

fn main() {
    let fields = sample().to_fields();

    // The per-leaf inverse maps each value back to its Scalar.
    assert_eq!(*scalar_of(&fields, "text"), Scalar::String("api".to_string()));
    assert_eq!(*scalar_of(&fields, "count"), Scalar::Int(42));
    assert_eq!(*scalar_of(&fields, "ratio"), Scalar::Float(0.5));
    assert_eq!(*scalar_of(&fields, "flag"), Scalar::Bool(true));
    // PathBuf has no scalar of its own, so it emits as a string.
    assert_eq!(
        *scalar_of(&fields, "path"),
        Scalar::String("/etc/app".to_string())
    );
    assert_eq!(*scalar_of(&fields, "defaulted"), Scalar::Int(9));

    // Optional leaves: present is emitted, absent is omitted.
    assert!(fields.get("opt_present").is_some());
    assert!(fields.get("opt_absent").is_none());

    // The string list is a sequence of string scalars.
    match &fields.get("list").expect("list present").kind {
        FieldKind::Value(Value {
            kind: ValueKind::Seq(elements),
            ..
        }) => assert_eq!(elements.len(), 2),
        _ => panic!("list is not a sequence"),
    }

    // The marked absent block is filled and the unmarked absent block is omitted.
    assert!(matches!(
        fields.get("marked_absent").map(|field| &field.kind),
        Some(FieldKind::Block(_))
    ));
    assert!(fields.get("unmarked_absent").is_none());

    // The nested list emits one Block per element, not one array attribute.
    let blocks: Vec<_> = fields
        .iter()
        .filter(|field| field.name == "block_list")
        .collect();
    assert_eq!(blocks.len(), 2);
    assert!(
        blocks
            .iter()
            .all(|field| matches!(field.kind, FieldKind::Block(_)))
    );

    // Every span the walk produced is detached.
    assert_all_detached(&fields);

    // The fixed point: populate adds defaults, so the first reparse fills the
    // marked block, and populating and reparsing that reparse changes nothing.
    let mut report = Report::new();
    let first = All::from_fields(&sample().to_fields(), &mut report).expect("reparse populated");
    let second = All::from_fields(&first.to_fields(), &mut report).expect("reparse again");
    assert!(!report.has_issues());
    assert_eq!(first, second);
    assert!(first.marked_absent.is_some());
    assert!(first.unmarked_absent.is_none());

    // A filled block fills its own marked absent child, so the fill reaches a
    // grandchild block two levels down from the root.
    let deep = DeepTop { mid: None }.to_fields();
    let mid = block_of(&deep, "mid");
    let leaf = block_of(mid, "leaf");
    assert_eq!(*scalar_of(leaf, "n"), Scalar::Int(5));

    // A zero-field spec populates to an empty level.
    let empty = NoFields {}.to_fields();
    assert_eq!(empty.iter().count(), 0);
}
