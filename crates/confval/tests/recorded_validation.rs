//! Attribute-driven validation. A `#[confval(range = ...)]` or
//! `#[confval(keywords = ...)]` on a scalar field is checked by the generated
//! `ValidateNested::validate_recorded`, so the attribute alone enforces the
//! constraint and the `Validate` body carries no line for it.

#![allow(clippy::unwrap_used, clippy::expect_used)]
use confval::prelude::*;

range_constraint!(PORT, i64, min: 1, max: 65535);
range_constraint!(LEVEL, i64, min: 0, max: 10);
range_constraint!(WORKERS, i64, min: 1, max: 512);
range_constraint!(MAX_BODY_MB, i64, min: 1, max: 1024);

keyword_enum!(Mode, {
    On  => "on",
    Off => "off",
});

keyword_enum!(LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

/// A root with a recorded range, a recorded keyword, an optional recorded range,
/// and a plain field with no attribute. Its `Validate` body is empty, so only
/// the generated `validate_recorded` enforces the recorded fields.
#[derive(confval::Spec)]
struct Server {
    #[confval(range = PORT)]
    port: Located<i64>,
    #[confval(keywords = Mode)]
    mode: Located<String>,
    #[confval(range = LEVEL)]
    level: Option<Located<i64>>,
    plain: Located<i64>,
}

impl Validate for Server {
    fn validate(&self, _report: &mut Report) {}
}

/// A root that declares its keyword sets on string lists rather than on
/// scalars. The
/// bare form is a plain `Vec`, and the optional form keeps the outer `Located`.
/// Its `Validate` body is empty, so only `validate_recorded` checks the
/// elements.
#[derive(confval::Spec)]
struct Tags {
    #[confval(keywords = Mode)]
    modes: Vec<Located<String>>,
    #[confval(keywords = LimitMode)]
    limits: Option<Located<Vec<Located<String>>>>,
}

impl Validate for Tags {
    fn validate(&self, _report: &mut Report) {}
}

/// A root holding the list-bearing block, so the list check is reached through
/// the generated traversal rather than by validating the block directly.
#[derive(confval::Spec)]
struct TagsRoot {
    #[confval(nested)]
    tags: Option<Located<Tags>>,
}

impl Validate for TagsRoot {
    fn validate(&self, _report: &mut Report) {}
}

/// A nested block whose only rule is a recorded range, so it is checked only
/// through its parent's descent once its `Validate` body is empty.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Limits {
    #[confval(default = 0, range = LEVEL)]
    level: Located<i64>,
}

impl Validate for Limits {
    fn validate(&self, _report: &mut Report) {}
}

/// A root holding a required nested block and a repeated one, both `Limits`.
#[derive(confval::Spec)]
struct Root {
    #[confval(nested)]
    limits: Located<Limits>,
    #[confval(nested)]
    many: Vec<Located<Limits>>,
}

impl Validate for Root {
    fn validate(&self, _report: &mut Report) {}
}

/// A root that breaks `descend`, with its own recorded field and a nested child
/// that also has one.
#[derive(confval::Spec)]
struct Gated {
    #[confval(range = PORT)]
    port: Located<i64>,
    #[confval(nested)]
    child: Located<Limits>,
}

impl Validate for Gated {
    fn validate(&self, _report: &mut Report) {}

    fn descend(&self) -> ControlFlow<()> {
        ControlFlow::Break(())
    }
}

/// A spec whose author left the manual check in place after adding the
/// attribute, so the field is checked twice.
#[derive(confval::Spec)]
struct Doubled {
    #[confval(range = PORT)]
    port: Located<i64>,
}

impl Validate for Doubled {
    fn validate(&self, report: &mut Report) {
        PORT.check_located(&self.port, "port", report);
    }
}

/// A spec with a plain field and no recording attribute, so nothing is checked.
#[derive(confval::Spec)]
struct Plain {
    value: Located<i64>,
}

impl Validate for Plain {
    fn validate(&self, _report: &mut Report) {}
}

/// A handwritten spec with a manual check and a handwritten `ValidateNested`
/// that does not override `validate_recorded`.
struct Handwritten {
    port: Located<i64>,
}

impl Validate for Handwritten {
    fn validate(&self, report: &mut Report) {
        PORT.check_located(&self.port, "port", report);
    }
}

impl ValidateNested for Handwritten {
    fn validate_nested(&self, _report: &mut Report) {}
}

/// Mirrors the `common` fixture's recorded fields: a top-level range and an
/// optional nested block whose range and keyword are enforced only through the
/// parent's descent. Every rule is recorded, so both `Validate` bodies are empty.
#[derive(confval::Spec)]
struct FixtureServer {
    #[confval(range = PORT)]
    port: Located<i64>,
    #[confval(default = 4, range = WORKERS)]
    workers: Located<i64>,
    #[confval(nested)]
    limits: Option<Located<FixtureLimits>>,
}

impl Validate for FixtureServer {
    fn validate(&self, _report: &mut Report) {}
}

#[derive(confval::Spec)]
#[confval(derive_default)]
struct FixtureLimits {
    #[confval(default = 16, range = MAX_BODY_MB)]
    max_body_mb: Located<i64>,
    #[confval(default = "enforce".to_string(), keywords = LimitMode)]
    mode: Located<String>,
}

impl Validate for FixtureLimits {
    fn validate(&self, _report: &mut Report) {}
}

/// Runs the whole validation walk and returns the report.
fn validate<T: Validate + ValidateNested>(spec: &T) -> Report {
    let mut report = Report::new();
    spec.validate_all(&mut report);
    report
}

/// The messages in a report, in order.
fn messages(report: &Report) -> Vec<&str> {
    report
        .issues()
        .iter()
        .map(|issue| issue.message.as_str())
        .collect()
}

fn tags(modes: &[&str], limits: Option<&[&str]>) -> Tags {
    Tags {
        modes: modes
            .iter()
            .map(|word| Located::detached(word.to_string()))
            .collect(),
        limits: limits.map(|words| {
            Located::detached(
                words
                    .iter()
                    .map(|word| Located::detached(word.to_string()))
                    .collect(),
            )
        }),
    }
}

fn server(port: i64, mode: &str, level: Option<i64>) -> Server {
    Server {
        port: Located::detached(port),
        mode: Located::detached(mode.to_string()),
        level: level.map(Located::detached),
        plain: Located::detached(0),
    }
}

#[test]
fn a_range_field_is_checked_without_a_manual_line() {
    // Arrange
    let spec = server(99999, "on", None);

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(messages(&report), vec!["port must be at most 65535"]);
}

#[test]
fn a_keyword_field_is_checked_without_a_manual_line() {
    // Arrange
    let spec = server(80, "nope", None);

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(messages(&report), vec!["unknown mode: nope"]);
}

#[test]
fn a_clean_value_reports_nothing() {
    // Arrange
    let spec = server(80, "on", Some(5));

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_issues());
}

#[test]
fn an_optional_range_field_is_skipped_when_absent() {
    // Arrange
    let absent = server(80, "on", None);

    // Act
    let report = validate(&absent);

    // Assert
    assert!(!report.has_issues());
}

#[test]
fn an_optional_range_field_is_checked_when_present() {
    // Arrange
    let present = server(80, "on", Some(99));

    // Act
    let report = validate(&present);

    // Assert
    assert_eq!(messages(&report), vec!["level must be at most 10"]);
}

#[test]
fn a_field_with_no_attribute_is_not_checked() {
    // Arrange
    let spec = Plain {
        value: Located::detached(i64::MAX),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_issues());
}

#[test]
fn a_recorded_field_in_a_nested_block_is_checked() {
    // Arrange
    let spec = Root {
        limits: Located::detached(Limits {
            level: Located::detached(50),
        }),
        many: Vec::new(),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(messages(&report), vec!["level must be at most 10"]);
}

#[test]
fn a_recorded_field_in_a_repeated_block_is_checked() {
    // Arrange
    let spec = Root {
        limits: Located::detached(Limits {
            level: Located::detached(0),
        }),
        many: vec![
            Located::detached(Limits {
                level: Located::detached(0),
            }),
            Located::detached(Limits {
                level: Located::detached(77),
            }),
        ],
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(messages(&report), vec!["level must be at most 10"]);
}

#[test]
fn a_broken_descend_runs_own_recorded_checks_but_skips_children() {
    // Arrange
    let spec = Gated {
        port: Located::detached(99999),
        child: Located::detached(Limits {
            level: Located::detached(50),
        }),
    };

    // Act
    let report = validate(&spec);

    // Assert
    // The block's own recorded field is checked, and the pruned child is not.
    assert_eq!(messages(&report), vec!["port must be at most 65535"]);
}

#[test]
fn a_leftover_manual_check_reports_twice() {
    // Arrange
    let spec = Doubled {
        port: Located::detached(99999),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(
        messages(&report),
        vec!["port must be at most 65535", "port must be at most 65535"]
    );
}

#[test]
fn a_handwritten_validate_nested_is_unaffected() {
    // Arrange
    let spec = Handwritten {
        port: Located::detached(99999),
    };

    // Act
    let report = validate(&spec);

    // Assert
    // The default empty `validate_recorded` adds nothing, so the manual check
    // fires once rather than being doubled.
    assert_eq!(messages(&report), vec!["port must be at most 65535"]);
}

#[test]
fn the_generated_check_reports_at_the_field_span() {
    // Arrange
    let mut sources = SourceMap::new();
    let id = sources.add("server.hcl", "port = 99999");
    let span = Span {
        source: id,
        start: 7,
        end: 12,
    };
    let spec = Server {
        port: Located { value: 99999, span },
        mode: Located::detached("on".to_string()),
        level: None,
        plain: Located::detached(0),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(report.issues()[0].message, "port must be at most 65535");
    assert_eq!(report.issues()[0].span, Some(span));
}

#[test]
fn the_migrated_fixture_reports_the_same_ordered_diagnostics() {
    // Arrange
    // A top-level range violation, and a populated nested block with a range and
    // a keyword violation. The recorded checks fire the whole set through one
    // `validate_all`: the top level first, then the descended block, in field
    // order.
    let mut sources = SourceMap::new();
    let id = sources.add("server.hcl", "port = 99999");
    let port_span = Span {
        source: id,
        start: 7,
        end: 12,
    };
    let spec = FixtureServer {
        port: Located {
            value: 99999,
            span: port_span,
        },
        workers: Located::detached(4),
        limits: Some(Located::detached(FixtureLimits {
            max_body_mb: Located::detached(9999),
            mode: Located::detached("nope".to_string()),
        })),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(
        messages(&report),
        vec![
            "port must be at most 65535",
            "max_body_mb must be at most 1024",
            "unknown mode: nope",
        ]
    );
    assert_eq!(report.issues()[0].span, Some(port_span));
}

#[derive(confval::Spec)]
struct BadDefault {
    #[confval(default = 99999, range = PORT)]
    port: Located<i64>,
}

impl Validate for BadDefault {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn a_default_that_violates_its_recorded_constraint_names_the_spec() {
    // Arrange
    // The operator never wrote the value, so the failure names the spec's
    // default rather than reporting a config error with no location.
    let spec = BadDefault {
        port: Located::detached(99999),
    };

    // Act
    let report = validate(&spec);

    // Assert
    let issue = report
        .issues()
        .iter()
        .find(|issue| {
            issue.message
                == "the default for `port` fails its recorded constraint: port must be at most 65535"
        })
        .unwrap_or_else(|| panic!("expected the spec-default error, got: {:?}", report.issues()));
    assert!(
        issue
            .help
            .as_deref()
            .is_some_and(|help| help.contains("#[confval(default = ...)]")),
        "the help points at the spec declaration: {:?}",
        issue.help
    );
}

#[test]
fn an_operator_value_keeps_the_ordinary_constraint_message() {
    // Arrange
    // A value the operator wrote carries a span and fails with the ordinary
    // message, even on a field that declares a default.
    let mut sources = SourceMap::new();
    let id = sources.add("server.toml", "port = 70000\n");
    let spec = BadDefault {
        port: Located::new(70000, confval::source::Span::new(id, 7, 12)),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(messages(&report), vec!["port must be at most 65535"]);
}

#[test]
fn every_element_of_a_bare_keyword_list_is_checked() {
    // Arrange
    let spec = tags(&["on", "nope", "off", "wat"], None);

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(
        messages(&report),
        vec![
            "unknown value in modes: nope",
            "unknown value in modes: wat"
        ]
    );
}

#[test]
fn every_element_of_an_optional_keyword_list_is_checked() {
    // Arrange
    let spec = tags(&[], Some(&["enforce", "shout"]));

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(messages(&report), vec!["unknown value in limits: shout"]);
}

#[test]
fn an_absent_keyword_list_reports_nothing() {
    // Arrange
    let spec = tags(&[], None);

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_issues());
}

#[test]
fn a_keyword_list_reports_at_the_offending_element() {
    // Arrange
    let mut sources = SourceMap::new();
    let id = sources.add("tags.hcl", "modes = [\"on\", \"nope\"]");
    let spec = Tags {
        modes: vec![
            Located {
                value: "on".to_string(),
                span: Span {
                    source: id,
                    start: 9,
                    end: 13,
                },
            },
            Located {
                value: "nope".to_string(),
                span: Span {
                    source: id,
                    start: 15,
                    end: 21,
                },
            },
        ],
        limits: None,
    };

    // Act
    let report = validate(&spec);

    // Assert
    // The span is the element's own, not the list's, so a diagnostic underlines
    // the one word the operator must change.
    let issue = &report.issues()[0];
    assert_eq!(
        issue.span.map(|span| (span.start, span.end)),
        Some((15, 21))
    );
}

#[test]
fn an_optional_keyword_list_present_but_empty_reports_nothing() {
    // Arrange
    let spec = tags(&[], Some(&[]));

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_issues());
}

#[test]
fn every_bad_element_of_an_optional_keyword_list_is_reported() {
    // Arrange
    let spec = tags(&[], Some(&["enforce", "shout", "log", "holler"]));

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(
        messages(&report),
        vec![
            "unknown value in limits: shout",
            "unknown value in limits: holler"
        ]
    );
}

#[test]
fn an_all_valid_keyword_list_reports_nothing() {
    // Arrange
    let spec = tags(&["on", "off"], Some(&["enforce", "log"]));

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_issues());
}

#[test]
fn an_optional_keyword_list_reports_at_the_offending_element() {
    // Arrange
    let mut sources = SourceMap::new();
    let id = sources.add("tags.hcl", "limits = [\"enforce\", \"shout\"]");
    let bad = Span {
        source: id,
        start: 21,
        end: 28,
    };
    let spec = Tags {
        modes: Vec::new(),
        limits: Some(Located {
            value: vec![
                Located::detached("enforce".to_string()),
                Located {
                    value: "shout".to_string(),
                    span: bad,
                },
            ],
            span: Span {
                source: id,
                start: 9,
                end: 29,
            },
        }),
    };

    // Act
    let report = validate(&spec);

    // Assert
    // The optional shape reaches its elements through the wrapper, so this pins
    // that the span is the element's own rather than the list's.
    assert_eq!(report.issues()[0].span, Some(bad));
}

#[test]
fn a_keyword_list_in_a_nested_block_is_reached_through_the_traversal() {
    // Arrange
    let spec = TagsRoot {
        tags: Some(Located::detached(tags(&["nope"], None))),
    };

    // Act
    let report = validate(&spec);

    // Assert
    // `TagsRoot` has no rules of its own, so this reports only if the generated
    // descent reaches the child's recorded list check.
    assert_eq!(messages(&report), vec!["unknown value in modes: nope"]);
}

/// A spec with a `non_empty` recorded field on a string leaf.
#[derive(confval::Spec)]
struct NonEmptyLeaf {
    #[confval(non_empty)]
    name: Located<String>,
}

impl Validate for NonEmptyLeaf {
    fn validate(&self, _report: &mut Report) {}
}

/// A spec with `non_empty` on a bare string list.
#[derive(confval::Spec)]
struct NonEmptyList {
    #[confval(non_empty)]
    tags: Vec<Located<String>>,
}

impl Validate for NonEmptyList {
    fn validate(&self, _report: &mut Report) {}
}

/// A spec combining `non_empty` with a value constraint on the same field.
#[derive(confval::Spec)]
struct NonEmptyWithKeywords {
    #[confval(non_empty, keywords = Mode)]
    mode: Located<String>,
}

impl Validate for NonEmptyWithKeywords {
    fn validate(&self, _report: &mut Report) {}
}

#[test]
fn non_empty_on_a_leaf_reports_an_empty_string() {
    // Arrange
    let spec = NonEmptyLeaf {
        name: Located::detached(String::new()),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(messages(&report), vec!["name must not be empty"]);
}

#[test]
fn non_empty_on_a_leaf_passes_a_non_empty_string() {
    // Arrange
    let spec = NonEmptyLeaf {
        name: Located::detached("api".to_string()),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_errors());
}

#[test]
fn non_empty_on_a_list_reports_each_empty_element() {
    // Arrange
    let spec = NonEmptyList {
        tags: vec![
            Located::detached("good".to_string()),
            Located::detached(String::new()),
            Located::detached("  ".to_string()),
        ],
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert_eq!(
        messages(&report),
        vec!["tags must not be empty", "tags must not be empty"]
    );
}

#[test]
fn non_empty_on_a_list_passes_when_all_elements_are_non_empty() {
    // Arrange
    let spec = NonEmptyList {
        tags: vec![
            Located::detached("a".to_string()),
            Located::detached("b".to_string()),
        ],
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_errors());
}

#[test]
fn non_empty_combines_with_a_value_constraint() {
    // Arrange
    let spec = NonEmptyWithKeywords {
        mode: Located::detached(String::new()),
    };

    // Act
    let report = validate(&spec);

    // Assert
    let msgs = messages(&report);
    assert!(
        msgs.contains(&"mode must not be empty"),
        "non_empty fires: {msgs:?}"
    );
    assert!(
        msgs.iter().any(|m| m.starts_with("unknown mode:")),
        "keywords fires: {msgs:?}"
    );
}

#[test]
fn non_empty_and_keywords_both_pass_a_valid_value() {
    // Arrange
    let spec = NonEmptyWithKeywords {
        mode: Located::detached("on".to_string()),
    };

    // Act
    let report = validate(&spec);

    // Assert
    assert!(!report.has_errors());
}
