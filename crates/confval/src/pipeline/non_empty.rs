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

