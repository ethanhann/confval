//! The generated traversal: what `Validate::validate_all` reaches from one
//! call at the root of a spec tree, and what a `descend` override prunes.
//!
//! Each spec type here reports a message naming itself. A report's contents
//! therefore read as the list of types the walk visited.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "derive")]

use confval::prelude::{ControlFlow, Located, Report, Validate};

#[derive(Default, confval::Spec)]
struct RootSpec {
    #[confval(nested)]
    required: Located<ChildSpec>,
    #[confval(nested)]
    optional: Option<Located<ChildSpec>>,
    #[confval(nested)]
    list: Vec<Located<ChildSpec>>,
}

impl Validate for RootSpec {
    fn validate(&self, report: &mut Report) {
        report.error("root").emit();
    }
}

#[derive(Default, confval::Spec)]
struct ChildSpec {
    name: Located<String>,
    #[confval(nested)]
    grandchild: Option<Located<GrandchildSpec>>,
}

impl Validate for ChildSpec {
    fn validate(&self, report: &mut Report) {
        report.error(format!("child {}", self.name.value)).emit();
    }
}

#[derive(Default, confval::Spec)]
struct GrandchildSpec {
    depth: Located<i64>,
}

impl Validate for GrandchildSpec {
    fn validate(&self, report: &mut Report) {
        report
            .error(format!("grandchild {}", self.depth.value))
            .emit();
    }
}

/// A spec whose children are only visited when the block is enabled.
#[derive(Default, confval::Spec)]
struct GatedSpec {
    enable: Located<bool>,
    #[confval(nested)]
    child: Located<ChildSpec>,
}

impl Validate for GatedSpec {
    fn validate(&self, report: &mut Report) {
        report.error("gated").emit();
    }

    fn descend(&self) -> ControlFlow<()> {
        if self.enable.value {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }
}

fn child(name: &str) -> ChildSpec {
    ChildSpec {
        name: Located::detached(name.to_string()),
        grandchild: None,
    }
}

fn messages(report: &Report) -> Vec<String> {
    report
        .issues()
        .iter()
        .map(|issue| issue.message.clone())
        .collect()
}

#[test]
fn validate_all_visits_every_nested_shape_and_recurses() {
    // Arrange
    let mut with_grandchild = child("optional");
    with_grandchild.grandchild = Some(Located::detached(GrandchildSpec {
        depth: Located::detached(3),
    }));
    let spec = RootSpec {
        required: Located::detached(child("required")),
        optional: Some(Located::detached(with_grandchild)),
        list: vec![
            Located::detached(child("list0")),
            Located::detached(child("list1")),
        ],
    };
    let mut report = Report::new();

    // Act
    spec.validate_all(&mut report);

    // Assert
    assert_eq!(
        messages(&report),
        vec![
            "root",
            "child required",
            "child optional",
            "grandchild 3",
            "child list0",
            "child list1",
        ],
        "every nested shape is visited, in field order, to full depth"
    );
}

#[test]
fn validate_all_skips_an_absent_optional_child_and_an_empty_list() {
    // Arrange
    let spec = RootSpec {
        required: Located::detached(child("required")),
        optional: None,
        list: Vec::new(),
    };
    let mut report = Report::new();

    // Act
    spec.validate_all(&mut report);

    // Assert
    assert_eq!(messages(&report), vec!["root", "child required"]);
}

#[test]
fn validate_alone_checks_the_root_and_no_child() {
    // Arrange
    let spec = RootSpec {
        required: Located::detached(child("required")),
        optional: Some(Located::detached(child("optional"))),
        list: vec![Located::detached(child("list0"))],
    };
    let mut report = Report::new();

    // Act
    spec.validate(&mut report);

    // Assert
    assert_eq!(
        messages(&report),
        vec!["root"],
        "validate holds one type's own rules, and the traversal reaches the children"
    );
}

#[test]
fn descend_break_prunes_the_subtree_but_keeps_the_block_s_own_rules() {
    // Arrange
    let spec = GatedSpec {
        enable: Located::detached(false),
        child: Located::detached(child("gated child")),
    };
    let mut report = Report::new();

    // Act
    spec.validate_all(&mut report);

    // Assert
    assert_eq!(
        messages(&report),
        vec!["gated"],
        "validate runs before descend, so breaking keeps what the block itself reported"
    );
}

#[test]
fn descend_continue_visits_the_subtree() {
    // Arrange
    let spec = GatedSpec {
        enable: Located::detached(true),
        child: Located::detached(child("gated child")),
    };
    let mut report = Report::new();

    // Act
    spec.validate_all(&mut report);

    // Assert
    assert_eq!(messages(&report), vec!["gated", "child gated child"]);
}
