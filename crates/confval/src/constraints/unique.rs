//! The unique constraint, `#[confval(unique)]`.
//!
//! A list is unique when no element repeats an earlier one, compared as the
//! exact string. The message is `duplicate value in {field}: "{value}"`,
//! reported at the repeated element's span with a related label at the
//! first occurrence. The first occurrence itself is not reported. Each
//! repeat keeps its own span, so `check_list` takes no list-level span
//! where `NON_EMPTY.check_list` does.

use crate::diagnostic::Report;
use crate::source::{Located, Span};
use std::collections::HashMap;

/// The unique check. [`UNIQUE`] is the rule with the generated help line.
/// [`with_help`](UniqueConstraint::with_help) builds a rule whose help line
/// replaces it, which the derive emits for `#[confval(unique(help = "..."))]`
/// and a handwritten spec declares as a const.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct UniqueConstraint {
    /// A help line that replaces the generated suggestion.
    pub help: Option<&'static str>,
}

/// The rule with the generated help line, which a bare `#[confval(unique)]`
/// and a handwritten check name.
pub const UNIQUE: UniqueConstraint = UniqueConstraint { help: None };

impl UniqueConstraint {
    /// A rule whose help line replaces the generated suggestion.
    pub const fn with_help(help: &'static str) -> Self {
        Self { help: Some(help) }
    }

    /// Reports `duplicate value in {field}: "{value}"` for each element that
    /// repeats an earlier one, at that element's span, with the first
    /// occurrence as a related span. The value is quoted so an empty or
    /// whitespace-only repeat is visible in the message.
    pub fn check_list(&self, values: &[Located<String>], field: &str, report: &mut Report) {
        let mut first_span: HashMap<&str, Span> = HashMap::new();
        for value in values {
            let Some(&first) = first_span.get(value.value.as_str()) else {
                first_span.insert(value.value.as_str(), value.span);
                continue;
            };
            let help = self
                .help
                .map(String::from)
                .unwrap_or_else(|| format!("Remove the repeated entry from {field}"));
            report
                .error(format!("duplicate value in {field}: \"{}\"", value.value))
                .at(value.span)
                .related(first, "first declared here")
                .help(help)
                .emit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceMap;

    #[test]
    fn a_list_with_no_repeat_passes() {
        // Arrange
        let values = vec![
            Located::detached("a".to_string()),
            Located::detached("b".to_string()),
        ];
        let mut report = Report::new();

        // Act
        UNIQUE.check_list(&values, "tags", &mut report);

        // Assert
        assert!(!report.has_issues());
    }

    #[test]
    fn a_provided_help_replaces_the_generated_line() {
        // Arrange
        let rule = UniqueConstraint::with_help("Each listener may appear once.");
        let values = vec![
            Located::detached("a".to_string()),
            Located::detached("a".to_string()),
        ];
        let mut report = Report::new();

        // Act
        rule.check_list(&values, "listeners", &mut report);

        // Assert
        assert_eq!(
            report.issues()[0].message,
            "duplicate value in listeners: \"a\""
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Each listener may appear once.")
        );
    }

    #[test]
    fn the_bare_rule_keeps_the_generated_help() {
        // Arrange
        let values = vec![
            Located::detached("a".to_string()),
            Located::detached("a".to_string()),
        ];
        let mut report = Report::new();

        // Act
        UNIQUE.check_list(&values, "tags", &mut report);

        // Assert
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Remove the repeated entry from tags")
        );
    }

    #[test]
    fn each_repeat_is_reported_at_its_own_span_and_the_first_is_not() {
        // Arrange
        let mut sources = SourceMap::new();
        let id = sources.add("test.hcl", "tags = [\"a\", \"a\", \"b\", \"a\"]");
        let first = Span::new(id, 8, 11);
        let second = Span::new(id, 13, 16);
        let third = Span::new(id, 23, 26);
        let values = vec![
            Located::new("a".to_string(), first),
            Located::new("a".to_string(), second),
            Located::detached("b".to_string()),
            Located::new("a".to_string(), third),
        ];
        let mut report = Report::new();

        // Act
        UNIQUE.check_list(&values, "tags", &mut report);

        // Assert
        assert_eq!(report.issues().len(), 2);
        assert_eq!(report.issues()[0].message, "duplicate value in tags: \"a\"");
        assert_eq!(report.issues()[0].span, Some(second));
        assert_eq!(
            report.issues()[0].related.first().map(|(span, _)| *span),
            Some(first),
            "the repeat points back at the first occurrence"
        );
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Remove the repeated entry from tags")
        );
        assert_eq!(report.issues()[1].span, Some(third));
    }

    #[test]
    fn the_comparison_is_the_exact_string() {
        // Arrange
        let values = vec![
            Located::detached("a".to_string()),
            Located::detached("A".to_string()),
            Located::detached("a ".to_string()),
        ];
        let mut report = Report::new();

        // Act
        UNIQUE.check_list(&values, "tags", &mut report);

        // Assert
        assert!(
            !report.has_issues(),
            "case and whitespace distinguish entries"
        );
    }

    #[test]
    fn a_repeated_empty_entry_is_quoted_in_the_message() {
        // Arrange
        let values = vec![
            Located::detached(String::new()),
            Located::detached(String::new()),
        ];
        let mut report = Report::new();

        // Act
        UNIQUE.check_list(&values, "tags", &mut report);

        // Assert
        assert_eq!(report.issues()[0].message, "duplicate value in tags: \"\"");
    }

    #[test]
    fn an_empty_list_passes() {
        // Arrange
        let values: Vec<Located<String>> = vec![];
        let mut report = Report::new();

        // Act
        UNIQUE.check_list(&values, "tags", &mut report);

        // Assert
        assert!(!report.has_issues());
    }
}
