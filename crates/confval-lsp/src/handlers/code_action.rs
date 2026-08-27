//! The code-action handler: the reset-to-default quick fix.
//!
//! A diagnostic contained in the value span of a scalar field that carries a
//! rendered default gets one quick fix: set the field to its default. The
//! default comes from the schema, so the fix resolves a constraint violation
//! and a type mismatch alike. The diagnostics come from the request's context,
//! the client's own published set, so the server recomputes nothing.

use std::collections::HashMap;

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, TextEdit, Uri, WorkspaceEdit,
};

use confval::LengthConstraint;
use confval::schema::{Constraint, SchemaType};

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::{Frontend, PositionKind};
use crate::handlers::Cx;
use crate::resolve::value_span_in;
use crate::walk::schema_at;

/// Produces the quick fixes for a resolved cursor.
///
/// The context resolves at the request range's start. The action is offered
/// only at a value position of a defaulted scalar, when a context diagnostic
/// is inside the parsed value span. A diagnostic anchored at an enclosing
/// block or the file produces no fix. `only` is the client's kind filter.
pub fn code_action<F: Frontend + ?Sized>(
    frontend: &F,
    cx: &Cx,
    diagnostics: &[Diagnostic],
    only: Option<&[CodeActionKind]>,
    uri: &Uri,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Vec<CodeActionOrCommand> {
    if !quickfix_requested(only) {
        return Vec::new();
    }
    let PositionKind::AttributeValue { field } = &cx.ctx.kind else {
        return Vec::new();
    };
    let Some(enclosing) = schema_at(cx.schema, &cx.ctx.path) else {
        return Vec::new();
    };
    let Some(target) = enclosing.fields.iter().find(|f| &f.name == field) else {
        return Vec::new();
    };
    let SchemaType::Scalar {
        leaf, constraint, ..
    } = &target.ty
    else {
        return Vec::new();
    };
    // A reference names another block's label, so its default, if any, is not
    // a value to reset to.
    if matches!(constraint, Some(Constraint::References { .. })) {
        return Vec::new();
    }
    let Some(text) = &target.default_text else {
        return Vec::new();
    };
    // The derive permits a default outside the field's own constraint. Such a
    // default is no fix, because applying it leaves a diagnostic in place.
    if !default_satisfies(constraint, text) {
        return Vec::new();
    }
    let Some(body) = &cx.ctx.resolved_body else {
        return Vec::new();
    };
    let Some(value_span) = value_span_in(body, field, cx.text) else {
        return Vec::new();
    };
    let contained: Vec<Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| contained_in(diagnostic, value_span, cx.text, index, encoding))
        .cloned()
        .collect();
    if contained.is_empty() {
        return Vec::new();
    }
    let literal = frontend.default_literal(leaf, text);
    let edit = TextEdit {
        range: index.range_of_bytes(cx.text, value_span, encoding),
        new_text: literal.clone(),
    };
    let action = CodeAction {
        title: format!("Set {field} to the default {}", shortened(&literal)),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(contained),
        edit: Some(WorkspaceEdit {
            changes: Some(HashMap::from([(uri.clone(), vec![edit])])),
            ..WorkspaceEdit::default()
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    };
    vec![CodeActionOrCommand::CodeAction(action)]
}

/// The literal bounded for a menu title. The edit keeps the whole text, and
/// only the title elides a long default.
fn shortened(literal: &str) -> String {
    const LIMIT: usize = 40;
    if literal.chars().count() <= LIMIT {
        return literal.to_string();
    }
    let head: String = literal.chars().take(LIMIT).collect();
    format!("{head}...")
}

/// Whether the client's kind filter admits a quick fix. An absent filter
/// admits everything, and a kind admits its own prefix tree.
fn quickfix_requested(only: Option<&[CodeActionKind]>) -> bool {
    let Some(kinds) = only else {
        return true;
    };
    kinds.iter().any(|kind| {
        let kind = kind.as_str();
        kind.is_empty() || CodeActionKind::QUICKFIX.as_str().starts_with(kind)
    })
}

/// Whether the rendered default passes the field's own constraint.
fn default_satisfies(constraint: &Option<Constraint>, text: &str) -> bool {
    match constraint {
        Some(Constraint::Keywords(words)) => words.contains(&text),
        Some(Constraint::Range { min, max, .. }) => {
            match (text.parse::<f64>(), min.parse::<f64>(), max.parse::<f64>()) {
                (Ok(value), Ok(min), Ok(max)) => min <= value && value <= max,
                _ => false,
            }
        }
        Some(Constraint::Length { min, max, .. }) => LengthConstraint {
            min: *min,
            max: *max,
            help: None,
        }
        .admits(text),
        Some(Constraint::Format { check, .. }) => check.call(text),
        _ => true,
    }
}

/// Whether a diagnostic's range is inside or equal to the value span. A
/// position beyond the current text is no containment, so a stale client
/// position cannot clamp onto a line end or the final offset and qualify.
fn contained_in(
    diagnostic: &Diagnostic,
    value_span: (usize, usize),
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> bool {
    let (Some(start), Some(end)) = (
        offset_within(diagnostic.range.start, text, index, encoding),
        offset_within(diagnostic.range.end, text, index, encoding),
    ) else {
        return false;
    };
    value_span.0 <= start && end <= value_span.1
}

/// The byte offset of a position that lies within the current text, or `None`
/// for a position the conversion had to clamp.
fn offset_within(
    position: lsp_types::Position,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<usize> {
    let offset = index.offset_of(text, position, encoding);
    let round_trip = index.range_of_bytes(text, (offset, offset), encoding).start;
    (round_trip == position).then_some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODING: PositionEncoding = PositionEncoding::Utf8;

    /// A diagnostic whose range covers the given byte span.
    fn diagnostic_over(text: &str, span: (usize, usize), index: &LineIndex) -> Diagnostic {
        Diagnostic {
            range: index.range_of_bytes(text, span, ENCODING),
            ..Diagnostic::default()
        }
    }

    #[test]
    fn contained_in_is_false_when_the_diagnostic_starts_before_the_value_span() {
        // Arrange
        let text = "0123456789\n";
        let index = LineIndex::new(text);
        let diagnostic = diagnostic_over(text, (1, 3), &index);

        // Act
        let contained = contained_in(&diagnostic, (4, 8), text, &index, ENCODING);

        // Assert
        assert!(
            !contained,
            "a diagnostic reaching left of the value span is not contained"
        );
    }

    #[test]
    fn contained_in_is_false_when_the_diagnostic_ends_after_the_value_span() {
        // Arrange
        let text = "0123456789\n";
        let index = LineIndex::new(text);
        let diagnostic = diagnostic_over(text, (5, 9), &index);

        // Act
        let contained = contained_in(&diagnostic, (4, 8), text, &index, ENCODING);

        // Assert
        assert!(
            !contained,
            "a diagnostic reaching right of the value span is not contained"
        );
    }

    #[test]
    fn contained_in_is_true_when_the_diagnostic_sits_inside_the_value_span() {
        // Arrange
        let text = "0123456789\n";
        let index = LineIndex::new(text);
        let diagnostic = diagnostic_over(text, (5, 7), &index);

        // Act
        let contained = contained_in(&diagnostic, (4, 8), text, &index, ENCODING);

        // Assert
        assert!(contained, "a diagnostic inside the value span is contained");
    }

    #[test]
    fn a_default_at_or_above_the_length_minimum_satisfies_the_constraint() {
        // Arrange
        let constraint = Some(Constraint::length(3, 10, None));

        // Act
        let satisfied = default_satisfies(&constraint, "abcd");

        // Assert
        assert!(
            satisfied,
            "a four-character default sits inside the 3 to 10 length range"
        );
    }

    #[test]
    fn a_default_below_the_length_minimum_fails_the_constraint() {
        // Arrange
        let constraint = Some(Constraint::length(3, 10, None));

        // Act
        let satisfied = default_satisfies(&constraint, "ab");

        // Assert
        assert!(
            !satisfied,
            "a two-character default is under the length minimum of 3"
        );
    }
}
