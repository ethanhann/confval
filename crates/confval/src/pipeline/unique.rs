//! The unique constraint, `#[confval(unique)]`.
//!
//! A list is unique when no element repeats an earlier one, compared as the
//! exact string. The message is `duplicate value in {field}: {value}`,
//! reported at the repeated element's span, so the first occurrence stays
//! where the operator wrote it and each later copy is named.

use crate::diagnostic::Report;
use crate::source::Located;
use std::collections::HashSet;

/// The unique check. Use the [`UNIQUE`] constant. Do not construct one.
#[derive(Debug, Clone, Copy)]
pub struct UniqueConstraint;

/// The single instance a caller or the derive names in a check call.
pub const UNIQUE: UniqueConstraint = UniqueConstraint;

impl UniqueConstraint {
    /// Reports `duplicate value in {field}: {value}` for each element that
    /// repeats an earlier one, at that element's span.
    pub fn check_list(&self, values: &[Located<String>], field: &str, report: &mut Report) {
        let mut seen = HashSet::new();
        for value in values {
            if seen.insert(value.value.as_str()) {
                continue;
            }
            report
                .error(format!("duplicate value in {field}: {}", value.value))
                .at(value.span)
                .help(format!("Remove the repeated entry from {field}"))
                .emit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceMap, Span};

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
        assert_eq!(report.issues()[0].message, "duplicate value in tags: a");
        assert_eq!(report.issues()[0].span, Some(second));
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
