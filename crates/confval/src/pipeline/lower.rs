use crate::diagnostic::Report;
use crate::source::Located;
use std::collections::{BTreeMap, HashMap};

/// Conversion from a Spec type to its runtime Config type.
///
/// The Spec is taken by reference because Specs are retained for history and
/// diffing. Failure returns `None` with the explanation already pushed to
/// the report.
///
/// Callers must gate on [`Report::has_errors`](crate::diagnostic::Report::has_errors)
/// before lowering. Lowering functions may assume field-level validation
/// passed, which is what makes narrowing conversions safe to write.
pub trait Lower<S>: Sized {
    /// Builds the runtime type from a validated spec, reporting anything
    /// that still fails. `None` means at least one error was pushed.
    fn lower(spec: &S, report: &mut Report) -> Option<Self>;
}

/// The auto-mapping backend for the `Config` derive: infallible unwrapping
/// of `Located` layers when the Spec and Config field share name and inner
/// type. Auto-mapped fields with incompatible types fail as a missing
/// `LowerAuto` implementation, naming both types.
///
/// Numeric narrowing is absent. The range that makes a cast
/// safe is knowledge this trait does not have, so narrowing always goes
/// through an explicit lowering function.
pub trait LowerAuto<Target> {
    /// The infallible conversion for an auto-mapped field.
    fn lower_auto(&self) -> Target;
}

impl<T: Clone> LowerAuto<T> for Located<T> {
    fn lower_auto(&self) -> T {
        self.value.clone()
    }
}

impl<T: Clone> LowerAuto<Option<T>> for Option<Located<T>> {
    fn lower_auto(&self) -> Option<T> {
        self.as_ref().map(|value| value.value.clone())
    }
}

impl<T: Clone> LowerAuto<Vec<T>> for Vec<Located<T>> {
    fn lower_auto(&self) -> Vec<T> {
        self.iter().map(|value| value.value.clone()).collect()
    }
}

impl<T: Clone> LowerAuto<Vec<T>> for Located<Vec<Located<T>>> {
    fn lower_auto(&self) -> Vec<T> {
        self.value.lower_auto()
    }
}

impl<T: Clone> LowerAuto<Option<Vec<T>>> for Option<Located<Vec<Located<T>>>> {
    fn lower_auto(&self) -> Option<Vec<T>> {
        self.as_ref().map(|list| list.value.lower_auto())
    }
}

/// A `#[confval(map)]` field's `BTreeMap<String, Located<V>>` lowers to a plain
/// `HashMap<String, V>`, dropping each value's span. The runtime map is what a
/// consumer reads, and a plain `HashMap` is the common runtime shape.
impl<V: Clone> LowerAuto<HashMap<String, V>> for BTreeMap<String, Located<V>> {
    fn lower_auto(&self) -> HashMap<String, V> {
        self.iter()
            .map(|(key, value)| (key.clone(), value.value.clone()))
            .collect()
    }
}

/// The same map lowers to a `BTreeMap<String, V>` for a consumer that wants a
/// sorted runtime map instead.
impl<V: Clone> LowerAuto<BTreeMap<String, V>> for BTreeMap<String, Located<V>> {
    fn lower_auto(&self) -> BTreeMap<String, V> {
        self.iter()
            .map(|(key, value)| (key.clone(), value.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The required wrapped list is the one shape `#[derive(Spec)]` rejects,
    /// pinned by `tests/ui/fail/spec_required_wrapped_string_list.rs`, so no
    /// derived spec reaches this impl and only a handwritten one can. The other
    /// four impls are covered through the derive in `tests/lower_auto.rs`.
    #[test]
    fn a_required_wrapped_list_keeps_every_element_in_order() {
        // Arrange
        let list = Located::detached(vec![
            Located::detached("edge".to_string()),
            Located::detached("beta".to_string()),
        ]);

        // Act
        let lowered: Vec<String> = list.lower_auto();

        // Assert
        assert_eq!(lowered, vec!["edge".to_string(), "beta".to_string()]);
    }

    #[test]
    fn a_required_wrapped_list_written_empty_lowers_to_an_empty_vec() {
        // Arrange
        let list: Located<Vec<Located<String>>> = Located::detached(Vec::new());

        // Act
        let lowered: Vec<String> = list.lower_auto();

        // Assert
        assert!(lowered.is_empty());
    }
}
