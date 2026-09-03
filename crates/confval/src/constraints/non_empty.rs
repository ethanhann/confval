//! The non-empty constraint, `#[confval(non_empty)]`.
//!
//! A string is empty when `value.trim().is_empty()`. A list is empty when it
//! holds no elements. The message is `{field} must not be empty` in both
//! cases, reported at the value's span.

use crate::diagnostic::Report;
use crate::source::{Located, Span};

/// The non-empty check. [`NON_EMPTY`] is the rule with the generated help
/// line. [`with_help`](NonEmptyConstraint::with_help) builds a rule whose help
/// line replaces it, which the derive emits for
/// `#[confval(non_empty(help = "..."))]` and a handwritten spec declares as a
/// const.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct NonEmptyConstraint {
    /// A help line that replaces the generated suggestion.
    pub help: Option<&'static str>,
}

/// The rule with the generated help line, which a bare `#[confval(non_empty)]`
/// and a handwritten check name.
pub const NON_EMPTY: NonEmptyConstraint = NonEmptyConstraint { help: None };

impl NonEmptyConstraint {
    /// A rule whose help line replaces the generated suggestion.
    pub const fn with_help(help: &'static str) -> Self {
        Self { help: Some(help) }
    }

    /// Reports `{field} must not be empty` when the string is empty or
    /// whitespace-only.
    pub fn check_located(&self, value: &Located<String>, field: &str, report: &mut Report) {
        if value.value.trim().is_empty() {
            let help = self
                .help
                .map(String::from)
                .unwrap_or_else(|| format!("Provide a non-empty value for {field}"));
            report
                .error(format!("{field} must not be empty"))
                .at(value.span)
                .help(help)
                .emit();
        }
    }

    /// Reports `{field} must not be empty` for each empty or whitespace-only
    /// element, at that element's span.
    pub fn check_each(&self, values: &[Located<String>], field: &str, report: &mut Report) {
        for value in values {
            self.check_located(value, field, report);
        }
    }

    /// Reports `{field} must not be empty` when the list holds no elements,
    /// at the given span.
    pub fn check_list(
        &self,
        values: &[Located<String>],
        field: &str,
        span: Span,
        report: &mut Report,
    ) {
        if values.is_empty() {
            let help = self
                .help
                .map(String::from)
                .unwrap_or_else(|| format!("Provide at least one item in {field}"));
            report
                .error(format!("{field} must not be empty"))
                .at(span)
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
    fn a_non_empty_string_passes() {
        // Arrange
        let value = Located::detached("hello".to_string());
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_located(&value, "name", &mut report);

        // Assert
        assert!(!report.has_errors());
    }

    #[test]
    fn an_empty_string_reports_an_error() {
        // Arrange
        let value = Located::detached(String::new());
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_located(&value, "name", &mut report);

        // Assert
        assert!(report.has_errors());
        assert_eq!(report.issues()[0].message, "name must not be empty");
    }

    #[test]
    fn a_whitespace_only_string_reports_an_error() {
        // Arrange
        let value = Located::detached("  \t\n  ".to_string());
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_located(&value, "hostname", &mut report);

        // Assert
        assert!(report.has_errors());
        assert_eq!(report.issues()[0].message, "hostname must not be empty");
    }

    #[test]
    fn check_each_reports_each_empty_element() {
        // Arrange
        let values = vec![
            Located::detached("good".to_string()),
            Located::detached(String::new()),
            Located::detached("  ".to_string()),
        ];
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_each(&values, "tag", &mut report);

        // Assert
        assert_eq!(report.issues().len(), 2);
        assert_eq!(report.issues()[0].message, "tag must not be empty");
        assert_eq!(report.issues()[1].message, "tag must not be empty");
    }

    #[test]
    fn a_provided_help_replaces_the_generated_line_on_a_leaf() {
        // Arrange
        let rule = NonEmptyConstraint::with_help("Provide the socket path.");
        let value = Located::detached(String::new());
        let mut report = Report::new();

        // Act
        rule.check_located(&value, "sock", &mut report);

        // Assert
        assert_eq!(report.issues()[0].message, "sock must not be empty");
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Provide the socket path.")
        );
    }

    #[test]
    fn a_provided_help_replaces_the_generated_line_on_a_list() {
        // Arrange
        let rule = NonEmptyConstraint::with_help("List at least one hook.");
        let mut report = Report::new();

        // Act
        rule.check_list(&[], "hooks", Span::detached(), &mut report);

        // Assert
        assert_eq!(report.issues()[0].message, "hooks must not be empty");
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("List at least one hook.")
        );
    }

    #[test]
    fn the_bare_rule_keeps_the_generated_help() {
        // Arrange
        let value = Located::detached(String::new());
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_located(&value, "name", &mut report);
        NON_EMPTY.check_list(&[], "tags", Span::detached(), &mut report);

        // Assert
        assert_eq!(
            report.issues()[0].help.as_deref(),
            Some("Provide a non-empty value for name")
        );
        assert_eq!(
            report.issues()[1].help.as_deref(),
            Some("Provide at least one item in tags")
        );
    }

    #[test]
    fn check_each_passes_when_all_elements_are_non_empty() {
        // Arrange
        let values = vec![
            Located::detached("a".to_string()),
            Located::detached("b".to_string()),
        ];
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_each(&values, "tag", &mut report);

        // Assert
        assert!(!report.has_errors());
    }

    #[test]
    fn an_empty_list_reports_an_error_at_the_span() {
        // Arrange
        let values: Vec<Located<String>> = vec![];
        let mut sources = SourceMap::new();
        let id = sources.add("test.hcl", "tags = []");
        let span = Span::new(id, 0, 9);
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_list(&values, "tags", span, &mut report);

        // Assert
        assert!(report.has_errors());
        assert_eq!(report.issues()[0].message, "tags must not be empty");
        assert_eq!(report.issues()[0].span, Some(span));
    }

    #[test]
    fn a_non_empty_list_passes() {
        // Arrange
        let values = vec![Located::detached("a".to_string())];
        let mut sources = SourceMap::new();
        let id = sources.add("test.hcl", "tags = [\"a\"]");
        let span = Span::new(id, 0, 12);
        let mut report = Report::new();

        // Act
        NON_EMPTY.check_list(&values, "tags", span, &mut report);

        // Assert
        assert!(!report.has_errors());
    }
}
