//! The rename handlers: the range a rename covers, and the edit that renames
//! a label and every reference that resolves to it.
//!
//! Both read the same label site as definition and references. The edit
//! covers the label value inside its quotes, so the author's quote style
//! survives. A scope that declares the label twice is refused, because the
//! validator already reports it and the edit would be ambiguous. A new name
//! that would break the literal is refused with a message the client shows.

use std::collections::HashMap;

use lsp_types::{PrepareRenameResponse, TextEdit, Uri, WorkspaceEdit};

use confval::schema::Schema;

use crate::encoding::{LineIndex, PositionEncoding};
use crate::frontend::CursorContext;

use super::{EditSite, LabelSite, Quote, edit_site, label_site, span_range};

/// The range a rename at the cursor would cover, or `None` when the cursor is
/// on nothing renameable.
pub fn prepare_rename(
    schema: &Schema,
    ctx: &CursorContext,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<PrepareRenameResponse> {
    let site = label_site(schema, ctx)?;
    if site.has_duplicate_label() {
        return None;
    }
    let cursor = ctx.token.0;
    let (_, under_cursor) = edit_sites(&site, text)?
        .into_iter()
        .find(|(span, _)| span.0 <= cursor && cursor <= span.1)?;
    Some(PrepareRenameResponse::Range(index.range_of_bytes(
        text,
        under_cursor.range,
        encoding,
    )))
}

/// The edit that renames the label under the cursor and every reference that
/// resolves to it.
///
/// `Ok(None)` is a cursor on nothing renameable. `Err` is a refused name, with
/// the reason.
pub fn rename(
    schema: &Schema,
    ctx: &CursorContext,
    uri: &Uri,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    new_name: &str,
) -> Result<Option<WorkspaceEdit>, String> {
    let Some(site) = label_site(schema, ctx) else {
        return Ok(None);
    };
    if site.has_duplicate_label() {
        return Ok(None);
    }
    let Some(edits) = edit_sites(&site, text) else {
        return Ok(None);
    };
    let edits: Vec<EditSite> = edits.into_iter().map(|(_, edit)| edit).collect();
    let new_name = check_name(new_name, &edits)?;
    let edits = edits
        .into_iter()
        .map(|edit| TextEdit {
            range: index.range_of_bytes(text, edit.range, encoding),
            new_text: new_name.to_string(),
        })
        .collect();
    Ok(Some(WorkspaceEdit {
        changes: Some(HashMap::from([(uri.clone(), edits)])),
        ..WorkspaceEdit::default()
    }))
}

/// Every site a rename edits, each with the parsed span it came from: the
/// declaration, then each reference. `None` when any site fails the value
/// check, because a partial rename would leave the document inconsistent.
fn edit_sites(site: &LabelSite, text: &str) -> Option<Vec<((usize, usize), EditSite)>> {
    let mut spans = Vec::new();
    spans.extend(site.declaration);
    spans.extend(site.reference_spans());
    if spans.is_empty() {
        return None;
    }
    spans
        .into_iter()
        .map(|span| {
            let range = span_range(span)?;
            Some((range, edit_site(text, range, &site.value)?))
        })
        .collect()
}

/// Checks a new name against the sites it is written into, and answers the
/// name as written.
///
/// A quote, a backslash, or a line break breaks every literal. A single quote
/// breaks a single-quoted site. A bare site takes an identifier only, because
/// a space, a colon, or a leading indicator byte would change the block or
/// the scalar type.
fn check_name<'a>(new_name: &'a str, edits: &[EditSite]) -> Result<&'a str, String> {
    let name = new_name.trim();
    if name.is_empty() {
        return Err("a label cannot be empty".to_string());
    }
    if name.contains(['"', '\\', '\n', '\r']) {
        return Err("a label cannot contain a quote, a backslash, or a line break".to_string());
    }
    if edits.iter().any(|edit| edit.quote == Quote::Single) && name.contains('\'') {
        return Err("a single-quoted label cannot contain a single quote".to_string());
    }
    if edits.iter().any(|edit| edit.quote == Quote::Bare) && !is_identifier(name) {
        return Err(
            "a bare label takes letters, digits, and `_.-` only, starting with a letter or `_`"
                .to_string(),
        );
    }
    Ok(name)
}

/// Whether a name is safe to write bare: `[A-Za-z_][A-Za-z0-9_.-]*`.
fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(quote: Quote) -> EditSite {
        EditSite {
            range: (0, 3),
            quote,
        }
    }

    #[test]
    fn a_name_that_breaks_a_literal_is_refused() {
        // Arrange
        let sites = [site(Quote::Double)];

        // Act
        let results = [
            check_name("  ", &sites),
            check_name("a\"b", &sites),
            check_name("a\\b", &sites),
            check_name("a\nb", &sites),
        ];

        // Assert
        assert!(results.iter().all(Result::is_err));
    }

    #[test]
    fn a_bare_site_takes_an_identifier_only() {
        // Arrange
        let bare = [site(Quote::Bare)];
        let quoted = [site(Quote::Double)];

        // Act
        let spaced_bare = check_name("my api", &bare);
        let spaced_quoted = check_name("my api", &quoted);
        let plain = check_name(" api-v2.x ", &bare);

        // Assert
        assert!(spaced_bare.is_err());
        assert_eq!(spaced_quoted, Ok("my api"));
        assert_eq!(plain, Ok("api-v2.x"));
    }

    #[test]
    fn a_single_quoted_site_refuses_a_single_quote() {
        // Arrange
        let single = [site(Quote::Single)];
        let double = [site(Quote::Double)];

        // Act
        let in_single = check_name("it's", &single);
        let in_double = check_name("it's", &double);

        // Assert
        assert!(in_single.is_err());
        assert_eq!(in_double, Ok("it's"));
    }
}
