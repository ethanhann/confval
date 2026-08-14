//! Position encoding over non-ASCII content.

use lsp_types::Position;

use confval_lsp::{LineIndex, PositionEncoding};

#[test]
fn a_non_ascii_value_converts_between_byte_offset_and_each_encoding() {
    // Arrange
    // The 'é' is two UTF-8 bytes and one UTF-16 code unit, so the character of
    // an offset after it differs by encoding.
    let text = "x = \"café\"\ny = 1\n";
    let index = LineIndex::new(text);
    let offset = text.find("\"\n").unwrap(); // the closing quote after café

    // Act
    let utf8 = index.position_of(text, offset, PositionEncoding::Utf8);
    let utf16 = index.position_of(text, offset, PositionEncoding::Utf16);

    // Assert
    assert_eq!(
        utf8,
        Position {
            line: 0,
            character: 10
        }
    );
    assert_eq!(
        utf16,
        Position {
            line: 0,
            character: 9
        }
    );
    // Each conversion round-trips back to the same byte offset.
    assert_eq!(index.offset_of(text, utf8, PositionEncoding::Utf8), offset);
    assert_eq!(
        index.offset_of(text, utf16, PositionEncoding::Utf16),
        offset
    );
}

#[test]
fn a_supplementary_plane_char_counts_two_utf16_code_units() {
    // Arrange
    // The '😀' is four UTF-8 bytes and a surrogate pair of two UTF-16 code
    // units, so the character of an offset after it differs by encoding.
    let text = "x = \"😀\"\ny = 1\n";
    let index = LineIndex::new(text);
    let offset = text.find("\"\n").unwrap(); // the closing quote after the emoji

    // Act
    let utf8 = index.position_of(text, offset, PositionEncoding::Utf8);
    let utf16 = index.position_of(text, offset, PositionEncoding::Utf16);

    // Assert
    assert_eq!(
        utf8,
        Position {
            line: 0,
            character: 9
        }
    );
    assert_eq!(
        utf16,
        Position {
            line: 0,
            character: 7
        }
    );
    assert_eq!(index.offset_of(text, utf8, PositionEncoding::Utf8), offset);
    assert_eq!(
        index.offset_of(text, utf16, PositionEncoding::Utf16),
        offset
    );
}

#[test]
fn the_start_of_an_empty_trailing_line_stays_on_that_line() {
    // Arrange
    // A blank line 2 begins at the end of the text, so its column 0 must resolve
    // to the end offset rather than clamp back onto line 1's newline.
    let text = "hostname = \"api\"\nport = 8080\n";
    let index = LineIndex::new(text);
    let position = Position {
        line: 2,
        character: 0,
    };

    // Act
    let offset = index.offset_of(text, position, PositionEncoding::Utf16);

    // Assert
    assert_eq!(offset, text.len());
}

#[test]
fn a_second_line_offset_resolves_its_line_and_column() {
    // Arrange
    let text = "hostname = \"api\"\nport = 8080\n";
    let index = LineIndex::new(text);
    let offset = text.find("8080").unwrap();

    // Act
    let position = index.position_of(text, offset, PositionEncoding::Utf16);

    // Assert
    assert_eq!(
        position,
        Position {
            line: 1,
            character: 7
        }
    );
    assert_eq!(
        index.offset_of(text, position, PositionEncoding::Utf16),
        offset
    );
}
