//! The LSP encoding of a raw completion item: the byte edit becomes a ranged
//! text edit under the negotiated encoding, and the snippet markers are kept
//! or stripped by the client's declared support.

use lsp_types::{CompletionItem, CompletionTextEdit, InsertTextFormat, TextEdit};

use crate::encoding::{LineIndex, PositionEncoding};

use super::{ClientSupport, RawItem};

/// Converts one raw item into the LSP shape: the byte edit becomes a ranged
/// text edit under the negotiated encoding.
///
/// A block insert has a `$0` tab stop. When the client supports snippets,
/// the edit is a snippet and the client places the cursor at the tab stop. When
/// it does not, the tab stop is removed so no literal `$0` reaches the buffer.
pub(super) fn encode_item(
    raw: RawItem,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
    client: ClientSupport,
) -> CompletionItem {
    // Only a producer-marked snippet is emitted or stripped as one, so a
    // literal value item passes through untouched in both directions.
    let is_snippet = client.snippets && raw.snippet;
    let new_text = if is_snippet || !raw.snippet {
        raw.new_text
    } else {
        crate::snippet::strip(&raw.new_text)
    };
    let new_text = reindent(new_text, text, raw.edit.0);
    let mut item = CompletionItem {
        label: raw.label,
        kind: Some(raw.kind),
        detail: raw.detail,
        filter_text: raw.filter_text,
        sort_text: Some(raw.sort_text),
        preselect: (raw.preselect && client.preselect).then_some(true),
        ..CompletionItem::default()
    };
    if is_snippet {
        item.insert_text_format = Some(InsertTextFormat::SNIPPET);
    }
    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
        range: index.range_of_bytes(text, raw.edit, encoding),
        new_text,
    }));
    item
}

/// Aligns the continuation lines of a multi-line insert with the column the
/// insert starts at. A frontend writes an insert relative to its own key, so
/// without the shift a nested completion's body lands at the buffer's left
/// margin, which puts a YAML body outside its block.
fn reindent(new_text: String, text: &str, start: usize) -> String {
    if !new_text.contains('\n') {
        return new_text;
    }
    let line_start = text[..start]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let column = text[line_start..start].chars().count();
    if column == 0 {
        return new_text;
    }
    let pad: String = format!("\n{}", " ".repeat(column));
    new_text.replace('\n', &pad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::CompletionItemKind;

    fn raw_item() -> RawItem {
        RawItem {
            label: "port".to_string(),
            kind: CompletionItemKind::FIELD,
            detail: Some("The listen port.".to_string()),
            filter_text: Some("port-filter".to_string()),
            sort_text: "0007".to_string(),
            preselect: true,
            snippet: false,
            edit: (0, 4),
            new_text: "port".to_string(),
        }
    }

    #[test]
    fn the_encoded_item_carries_every_populated_field() {
        // Arrange
        let text = "port: 8080";
        let index = LineIndex::new(text);
        let client = ClientSupport {
            snippets: true,
            preselect: true,
        };

        // Act
        let item = encode_item(raw_item(), text, &index, PositionEncoding::Utf8, client);

        // Assert
        assert_eq!(item.label, "port");
        assert_eq!(item.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(item.detail, Some("The listen port.".to_string()));
        assert_eq!(item.filter_text, Some("port-filter".to_string()));
        assert_eq!(item.sort_text, Some("0007".to_string()));
        assert_eq!(item.preselect, Some(true));
    }
}
