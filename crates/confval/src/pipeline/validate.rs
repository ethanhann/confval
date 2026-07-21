use crate::diagnostic::Report;

/// Field-local semantic validation for a Spec type.
///
/// A `Validate` impl checks what a value can prove about itself from its own
/// fields: ranges, closed-set keywords, formats, each reported at the span
/// the offending field already carries. Checks that need an enclosing span,
/// cross-field structure, or sibling context do not belong here; they live in
/// the central validators that hold the surrounding `Located` wrappers.
///
/// The trait exists so the requirement can be written as a bound.
/// `#[derive(Config)]` emits `impl Lower<S> ... where S: Validate`
/// to make sure the target spec of the lowering has a validator.
///
/// An empty impl satisfies the bound.
/// What it catches is the forgotten validator, not an unchecked field.
/// Validation is still invoked explicitly before the lowering gate, so the
/// bound does not make lowering call it.
pub trait Validate {
    fn validate(&self, report: &mut Report);
}
