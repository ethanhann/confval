use crate::diagnostic::Report;
use core::ops::ControlFlow;

/// Field-local semantic validation for a Spec type.
///
/// A `Validate` impl checks what a value can prove about itself from its own
/// fields: ranges, closed-set keywords, formats, each reported at the span
/// the offending field already carries. Checks that need an enclosing span,
/// cross-field structure, or sibling context do not belong here.
/// They are in the central validators that hold the surrounding `Located` wrappers.
///
/// The trait exists so the requirement can be written as a bound.
/// `#[derive(Config)]` emits `impl Lower<S> ... where S: Validate + ValidateNested`
/// to make sure the target spec of the lowering has a validator and a traversal.
///
/// An empty impl satisfies the `Validate` half of that bound.
/// It catches the forgotten validator rather than proving any field is checked.
/// Validation is still invoked explicitly before the lowering gate, so the
/// bound does not make lowering call it.
///
/// # `validate` writes the rules, `validate_all` runs them
///
/// [`validate`](Validate::validate) holds the rules for one spec type's own
/// fields and nothing else.
/// Implement it, but in normal use let
/// [`validate_all`](Validate::validate_all) call it rather than calling it
/// directly.
///
/// [`validate_all`](Validate::validate_all) is the entry point.
/// It runs `validate` on this type and then descends into every
/// `#[confval(nested)]` field, recursively, using the traversal that
/// `#[derive(Spec)]` generates.
/// Calling `validate` at the top of a pipeline checks the root and leaves
/// every nested block unchecked.
/// Closing that gap is what the traversal is for.
///
/// `validate_all` also runs the recorded constraint checks through
/// [`validate_recorded`](ValidateNested::validate_recorded), so a scalar field
/// that declares `#[confval(range = ...)]` or `#[confval(keywords = ...)]` is
/// checked without a line in `validate`. `validate` then holds only the rules an
/// attribute cannot express, such as a cross-field rule or a keyword list.
///
/// ```ignore
/// // The range is recorded on the field, so the derive checks it and the
/// // `Validate` body carries no line for it.
/// #[derive(confval::Spec)]
/// struct LimitsSpec {
///     #[confval(range = MAX_BODY_MB)]
///     max_body_mb: Located<i64>,
/// }
///
/// impl Validate for LimitsSpec {
///     fn validate(&self, _report: &mut Report) {}
/// }
///
/// // Called once, at the top of the pipeline. It runs the recorded check,
/// // then `validate`, then descends into every nested block.
/// spec.validate_all(&mut report);
/// ```
pub trait Validate {
    /// The rules for this type's own fields.
    ///
    /// Implement this and let [`validate_all`](Validate::validate_all) reach
    /// the nested children.
    /// A `validate` impl that calls a child's validator by hand reports that
    /// child's issues twice.
    fn validate(&self, report: &mut Report);

    /// Whether nested children are validated after `validate` returns.
    ///
    /// The default continues.
    /// A spec type that overrides nothing has its whole subtree checked.
    ///
    /// Override with `ControlFlow::Break(())` for a block that has declared
    /// itself inapplicable, where child diagnostics would be noise rather
    /// than help.
    /// A disabled feature whose sub-blocks no longer mean anything is the
    /// usual case.
    /// A version gate that has already reported the config targets a
    /// different schema is another.
    ///
    /// `descend` runs after `validate`.
    /// Whatever the rules reported about this block survives the pruning of
    /// the subtree.
    fn descend(&self) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// Validates this value and, unless [`descend`](Validate::descend) breaks,
    /// every nested child beneath it.
    ///
    /// This is the method to call.
    /// One call at the top of the pipeline covers the whole spec tree, because
    /// the generated [`ValidateNested`] traversal calls `validate_all` on each
    /// child in turn.
    ///
    /// The `Self: ValidateNested` bound ties the two halves together.
    /// `#[derive(Spec)]` satisfies it by writing the traversal from the struct
    /// definition.
    /// A nested block added tomorrow is therefore descended into without
    /// anyone editing a validator.
    fn validate_all(&self, report: &mut Report)
    where
        Self: ValidateNested,
    {
        self.validate_recorded(report);
        self.validate(report);
        if let ControlFlow::Continue(()) = self.descend() {
            self.validate_nested(report);
        }
    }
}

/// The generated traversal of a spec type's `#[confval(nested)]` fields.
///
/// A handwritten `Validate` impl carries two responsibilities: the rules for
/// this type's own fields, and a call into every nested child so the children
/// are checked too.
/// The second one is invisible when it is missing.
/// Adding a nested block to a spec leaves the parent's `Validate` impl
/// compiling and quietly skipping the new child.
/// The same omission on the lowering side is a compile error, because the
/// generated destructure is exhaustive.
///
/// Splitting the two responsibilities removes the omission.
/// `Validate` holds what only the author knows.
/// `#[derive(Spec)]` writes this impl from the struct definition, reading the
/// same field shapes it already reads to build the parser:
///
/// - `Located<S>` validates the value directly.
/// - `Option<Located<S>>` validates only when present.
/// - `Vec<Located<S>>` validates every element.
///
/// Non-nested fields are skipped.
/// A scalar is checked by the type's own `Validate` impl or not checked.
/// The derive cannot make that judgment.
///
/// Implement this by hand only alongside a handwritten `FromFields`, where
/// there is no `#[derive(Spec)]` to generate it.
/// An empty impl is the right answer for a spec type with no nested fields.
pub trait ValidateNested {
    /// Runs [`Validate::validate_all`] on every nested child of this value.
    fn validate_nested(&self, report: &mut Report);

    /// Runs this level's own recorded constraint checks, the ones a scalar field
    /// declares with `#[confval(range = ...)]` or `#[confval(keywords = ...)]`.
    ///
    /// The default runs nothing, so a handwritten impl keeps its checks in its
    /// own `Validate` body and is unaffected. `#[derive(Spec)]` overrides it for
    /// a spec that has a recorded field, so the attribute alone enforces the
    /// constraint and the `Validate` body carries no line for it.
    ///
    /// [`validate_all`](Validate::validate_all) calls this beside `validate`, so
    /// it runs on the type's own fields whether or not `descend` prunes the
    /// children.
    fn validate_recorded(&self, _report: &mut Report) {}
}
