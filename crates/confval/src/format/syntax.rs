//! Shared support for the format frontends: the one diagnostic every parser
//! produces the same way.
//!
//! A frontend wraps its parser's own error text in one confval issue. That text
//! comes from a third-party crate, so its wording is outside this crate's
//! control, and the crates disagree on whether a message opens uppercase.
//! Settling it at this boundary means an operator reads the same shape whichever
//! format they wrote.

/// The message one parser's syntax error reports.
///
/// The parser's own text follows `syntax error: `, with its first character
/// lowercased so the whole reads as one sentence. Lowercasing text that is
/// already lowercase changes nothing, so a parser that writes lowercase pays
/// only the call, and one that changes case in an upgrade cannot regress the
/// prefix.
///
/// A parser that reports no text still names the failure class, so the
/// prefix stands alone.
pub(crate) fn syntax_error(message: &str) -> String {
    let mut characters = message.chars();
    match characters.next() {
        Some(first) => {
            let rest: String = first.to_lowercase().chain(characters).collect();
            format!("syntax error: {rest}")
        }
        None => "syntax error".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uppercase_message_reads_as_one_sentence() {
        // Arrange
        // kdl-rs and jsonc-parser both write a standalone sentence.
        let message = "Expected end of document";

        // Act
        let reported = syntax_error(message);

        // Assert
        assert_eq!(reported, "syntax error: expected end of document");
    }

    #[test]
    fn a_lowercase_message_passes_through_unchanged() {
        // Arrange
        // toml_edit, hcl-edit, and saphyr-parser all write lowercase.
        let message = "unclosed table, expected `]`";

        // Act
        let reported = syntax_error(message);

        // Assert
        assert_eq!(reported, "syntax error: unclosed table, expected `]`");
    }

    #[test]
    fn only_the_first_character_changes_case() {
        // Arrange
        // A message naming a format or a type must keep the rest of its case.
        let message = "Invalid TOML value in `Foo`";

        // Act
        let reported = syntax_error(message);

        // Assert
        assert_eq!(reported, "syntax error: invalid TOML value in `Foo`");
    }

    #[test]
    fn an_empty_message_still_names_the_failure_class() {
        // Arrange
        // A kdl-rs diagnostic can have no message.
        let message = "";

        // Act
        let reported = syntax_error(message);

        // Assert
        assert_eq!(reported, "syntax error");
    }
}
