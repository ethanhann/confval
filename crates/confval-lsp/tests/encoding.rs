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
