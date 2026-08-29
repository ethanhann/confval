//! The byte range a rename or a highlight covers at a label site.

use confval::source::Span;

/// The quote a label site is written with, or none for a bare label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Quote {
    Double,
    Single,
    Bare,
}

/// The byte range an edit or a highlight covers at a label site. The range is
/// the label value itself, inside its quotes when it has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EditSite {
    pub(super) range: (usize, usize),
    pub(super) quote: Quote,
}

/// Anchors a site's range on the label value rather than on the span end.
///
/// A parsed span covers the quotes. A YAML quoted scalar's span runs to the
/// end of its line, past trailing spaces and a comment. When the byte at the
/// span start is a quote, the value must follow it and the same quote must
/// close it. Otherwise the value must start at the span start. The rest of
/// the span must then be blank or a `#` comment. An escaped string, a KDL raw
/// string, and a TOML multi-line string fail the check. None of them is
/// renameable.
pub(super) fn edit_site(text: &str, span: (usize, usize), value: &str) -> Option<EditSite> {
    let bytes = text.as_bytes();
    let (start, end) = (span.0, span.1.min(text.len()));
    if start >= end || value.is_empty() {
        return None;
    }
    let quote = match bytes[start] {
        b'"' => Quote::Double,
        b'\'' => Quote::Single,
        _ => Quote::Bare,
    };
    if quote == Quote::Bare {
        let range = (start, start + value.len());
        if text.get(range.0..range.1)? != value {
            return None;
        }
        let rest = text.get(range.1..end)?.trim_start();
        return (rest.is_empty() || rest.starts_with('#')).then_some(EditSite { range, quote });
    }
    let range = (start + 1, start + 1 + value.len());
    if text.get(range.0..range.1)? != value {
        return None;
    }
    (bytes.get(range.1) == Some(&bytes[start])).then_some(EditSite { range, quote })
}

/// A span's byte range, or `None` for the detached sentinel.
pub(super) fn span_range(span: Span) -> Option<(usize, usize)> {
    (!span.is_detached()).then_some((span.start as usize, span.end as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_site_strips_a_matching_pair_of_quotes() {
        // Arrange
        let text = "name = \"api\"  # c\n";

        // Act
        let site = edit_site(text, (7, 19), "api");

        // Assert
        assert_eq!(
            site,
            Some(EditSite {
                range: (8, 11),
                quote: Quote::Double
            })
        );
    }

    #[test]
    fn edit_site_accepts_a_bare_value_followed_by_a_comment() {
        // Arrange
        let text = "name: api   # c\n";

        // Act
        let site = edit_site(text, (6, 15), "api");

        // Assert
        assert_eq!(
            site,
            Some(EditSite {
                range: (6, 9),
                quote: Quote::Bare
            })
        );
    }

    #[test]
    fn edit_site_refuses_an_escaped_value_and_a_raw_string() {
        // Arrange
        let escaped = "name = \"a\\\"b\"\n";
        let raw = "upstream #\"raw\"# {}\n";

        // Act
        let escaped_site = edit_site(escaped, (7, 14), "a\"b");
        let raw_site = edit_site(raw, (9, 16), "raw");

        // Assert
        assert_eq!(escaped_site, None);
        assert_eq!(raw_site, None);
    }
}
