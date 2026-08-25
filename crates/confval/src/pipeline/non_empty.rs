//! The non_empty constraint, `#[confval(non_empty)]`.

use crate::diagnostic::Report;
use crate::prelude::Located;

#[derive(Debug, Clone)]
pub struct NonEmpty;

impl NonEmpty {
    pub const fn new() -> Self {
        Self
    }

    pub fn check_located_str(&self, value: &Located<&str>, field: &'static str, report: &mut Report)
    {
        if value.value.is_empty() {
            report
                .error(format!("{field} must not be empty"))
                .at(value.span)
                .help(format!("Provide a non-empty value for {field}"))
                .emit();
        }
    }

    pub fn check_located_vec<T>(
        &self,
        value: &Located<Vec<T>>,
        field: &'static str,
        report: &mut Report,
    ) where
        T: AsRef<str>,
    {
        if value.value.is_empty() {
            report
                .error(format!("{field} must not be empty"))
                .at(value.span)
                .help(format!("Provide at least one item in {field}"))
                .emit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_verify_string_is_empty() {
        // Arrange
        let value = Located::detached("");
        let subject = NonEmpty::new();
        let field = "foo";
        let mut report = Report::new();

        // Act
        subject.check_located_str(&value, field, &mut report);

        // Assert
        assert!(report.has_errors());
        assert_eq!(report.issues()[0].message, String::from("foo must not be empty"));
    }

    #[test]
    fn should_verify_string_is_not_empty() {
        // Arrange
        let value = Located::detached("something");
        let subject = NonEmpty::new();
        let field = "foo";
        let mut report = Report::new();

        // Act
        subject.check_located_str(&value, field, &mut report);

        // Assert
        assert!(!report.has_errors());
    }

    #[test]
    fn should_verify_empty_vec_is_empty() {
        // Arrange
        let value: Located<Vec<String>> = Located::detached(vec![]);
        let subject = NonEmpty::new();
        let field = "bar";
        let mut report = Report::new();

        // Act
        subject.check_located_vec(&value, field, &mut report);

        // Assert
        assert!(report.has_errors());
        assert_eq!(report.issues()[0].message, String::from("bar must not be empty"));
    }

    #[test]
    fn should_verify_vec_is_not_empty() {
        // Arrange
        let value = Located::detached(vec!["item1".to_string(), "item2".to_string()]);
        let subject = NonEmpty::new();
        let field = "bar";
        let mut report = Report::new();

        // Act
        subject.check_located_vec(&value, field, &mut report);

        // Assert
        assert!(!report.has_errors());
    }
}