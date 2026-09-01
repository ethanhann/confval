//! The rename handlers: the range a rename covers, and the edit that renames
//! a label and every reference that resolves to it.
//!
//! Both read the same label site as definition and references. The edit
//! covers the label value inside its quotes. The author's quote style stays.
//! A scope that declares the label twice is not renameable. The handler
//! refuses three kinds of rename: a new name that would break the value, a
//! name the scope already declares, and a site written as a raw or escaped
//! string. Each refusal answers a message the client shows. A rename from a
//! reference that resolves to no label edits the references that share its
//! value.

use std::collections::HashMap;

use lsp_types::{PrepareRenameResponse, TextEdit, Uri, WorkspaceEdit};

use crate::encoding::{LineIndex, PositionEncoding};
use crate::handlers::Cx;

use super::{EditSite, LabelSite, Quote, edit_site, label_site, span_range};

/// The range a rename at the cursor would cover, or `None` when the cursor is
/// on nothing renameable.
pub fn prepare_rename(
    cx: &Cx,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<PrepareRenameResponse> {
    let site = label_site(cx.schema, cx.ctx)?;
    if site.has_duplicate_label() {
        return None;
    }
    let cursor = cx.ctx.token.0;
    let (_, under_cursor) = edit_sites(&site, cx.text)
        .into_iter()
        .find(|(span, _)| span.0 <= cursor && cursor <= span.1)?;
    let under_cursor = under_cursor?;
    Some(PrepareRenameResponse::Range(index.range_of_bytes(
        cx.text,
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
    cx: &Cx,
    uri: &Uri,
    index: &LineIndex,
    encoding: PositionEncoding,
    new_name: &str,
) -> Result<Option<WorkspaceEdit>, String> {
    let Some(site) = label_site(cx.schema, cx.ctx) else {
        return Ok(None);
    };
    if site.has_duplicate_label() {
        return Ok(None);
    }
    let sites = edit_sites(&site, cx.text);
    if sites.is_empty() {
        return Ok(None);
    }
    let Some(edits) = sites
        .into_iter()
        .map(|(_, edit)| edit)
        .collect::<Option<Vec<EditSite>>>()
    else {
        return Err(
            "a label or a reference is written as a raw or escaped string and cannot be rewritten"
                .to_string(),
        );
    };
    let new_name = check_name(new_name, &edits)?;
    // A rename that edits no label leaves the declarations alone, so renaming
    // an unresolved reference onto an existing label stays allowed.
    if site.declaration.is_some() && site.declares_other_label(new_name) {
        return Err(format!("the scope already declares `{new_name}`"));
    }
    let edits = edits
        .into_iter()
        .map(|edit| TextEdit {
            range: index.range_of_bytes(cx.text, edit.range, encoding),
            new_text: new_name.to_string(),
        })
        .collect();
    Ok(Some(WorkspaceEdit {
        changes: Some(HashMap::from([(uri.clone(), edits)])),
        ..WorkspaceEdit::default()
    }))
}

/// Every site a rename edits, each with the parsed span it came from: the
/// declaration, then each reference. A site that fails the value check is
/// `None`, and the caller refuses the whole edit, because a partial rename
/// would leave the document inconsistent.
fn edit_sites(site: &LabelSite, text: &str) -> Vec<((usize, usize), Option<EditSite>)> {
    let mut spans = Vec::new();
    spans.extend(site.declaration);
    spans.extend(site.reference_spans());
    spans
        .into_iter()
        .filter_map(span_range)
        .map(|range| (range, edit_site(text, range, &site.value)))
        .collect()
}

/// Checks a new name against the sites it is written into, and answers the
/// name as written.
///
/// A quote, a backslash, a control character, or an HCL template opener
/// breaks every value. A single quote breaks a single-quoted site. A bare
/// site takes a plain name only. A space or a colon would change the block
/// or the scalar, and the literal words `true`, `false`, `null`, `inf`, and
/// `nan` would change the scalar type in KDL and YAML.
fn check_name<'a>(new_name: &'a str, edits: &[EditSite]) -> Result<&'a str, String> {
    let name = new_name.trim();
    if name.is_empty() {
        return Err("a label cannot be empty".to_string());
    }
    if name.contains(['"', '\\'])
        || name.chars().any(char::is_control)
        || name.contains("${")
        || name.contains("%{")
    {
        return Err(
            "a label cannot contain a quote, a backslash, a control character, `${`, or `%{`"
                .to_string(),
        );
    }
    if edits.iter().any(|edit| edit.quote == Quote::Single) && name.contains('\'') {
        return Err("a single-quoted label cannot contain a single quote".to_string());
    }
    if edits.iter().any(|edit| edit.quote == Quote::Bare) {
        if !is_plain_name(name) {
            return Err(
                "a bare label takes letters, digits, `_`, and `-` only, starting with a letter or `_`"
                    .to_string(),
            );
        }
        if LITERAL_WORDS.contains(&name.to_ascii_lowercase().as_str()) {
            return Err(format!("a bare label cannot be the literal word `{name}`"));
        }
    }
    Ok(name)
}

/// The words a bare KDL or YAML scalar reads as a bool, a null, or a float.
const LITERAL_WORDS: [&str; 5] = ["true", "false", "null", "inf", "nan"];

/// Whether a name is safe to write bare: `[A-Za-z_][A-Za-z0-9_-]*`, the class
/// the KDL and YAML emitters write without quotes.
fn is_plain_name(name: &str) -> bool {
    let Some((first, rest)) = name.as_bytes().split_first() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && *first != b'_' {
        return false;
    }
    rest.iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
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
    fn a_name_that_breaks_a_value_is_refused_with_the_reason() {
        // Arrange
        let sites = [site(Quote::Double)];
        let reason =
            "a label cannot contain a quote, a backslash, a control character, `${`, or `%{`";

        // Act
        let results = [
            check_name("  ", &sites),
            check_name("a\"b", &sites),
            check_name("a\\b", &sites),
            check_name("a\nb", &sites),
            check_name("a\tb", &sites),
            check_name("${x}", &sites),
        ];

        // Assert
        assert_eq!(results[0], Err("a label cannot be empty".to_string()));
        for result in &results[1..] {
            assert_eq!(*result, Err(reason.to_string()));
        }
    }

    #[test]
    fn a_bare_site_takes_a_plain_name_only() {
        // Arrange
        let bare = [site(Quote::Bare)];
        let quoted = [site(Quote::Double)];
        let class =
            "a bare label takes letters, digits, `_`, and `-` only, starting with a letter or `_`";

        // Act
        let spaced_bare = check_name("my api", &bare);
        let dotted_bare = check_name("api.v2", &bare);
        let digit_bare = check_name("1x", &bare);
        let literal_bare = check_name("True", &bare);
        let spaced_quoted = check_name("my api", &quoted);
        let literal_quoted = check_name("true", &quoted);
        let plain = check_name(" api-v2 ", &bare);
        let underscore = check_name("_x", &bare);

        // Assert
        assert_eq!(spaced_bare, Err(class.to_string()));
        assert_eq!(dotted_bare, Err(class.to_string()));
        assert_eq!(digit_bare, Err(class.to_string()));
        assert_eq!(underscore, Ok("_x"));
        assert_eq!(
            literal_bare,
            Err("a bare label cannot be the literal word `True`".to_string())
        );
        assert_eq!(spaced_quoted, Ok("my api"));
        assert_eq!(literal_quoted, Ok("true"));
        assert_eq!(plain, Ok("api-v2"));
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
        assert_eq!(
            in_single,
            Err("a single-quoted label cannot contain a single quote".to_string())
        );
        assert_eq!(in_double, Ok("it's"));
    }
}
