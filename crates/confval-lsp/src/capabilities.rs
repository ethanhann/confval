//! The initialize handshake helpers: encoding negotiation and the advertised
//! server capabilities.

use lsp_types::{
    CompletionOptions, HoverProviderCapability, InitializeParams, PositionEncodingKind,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
};

use crate::encoding::PositionEncoding;

/// Chooses the position encoding from the client's declared support, preferring
/// UTF-8 when the client offers it and falling back to the UTF-16 default.
pub(crate) fn negotiate(params: &InitializeParams) -> PositionEncoding {
    let supported = params
        .capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref());
    match supported {
        Some(kinds) if kinds.contains(&PositionEncodingKind::UTF8) => PositionEncoding::Utf8,
        _ => PositionEncoding::Utf16,
    }
}

/// The server's advertised capabilities.
pub(crate) fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding_kind(encoding)),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions::default()),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    }
}

/// The LSP encoding kind for a negotiated encoding.
fn encoding_kind(encoding: PositionEncoding) -> PositionEncodingKind {
    match encoding {
        PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
        PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{ClientCapabilities, GeneralClientCapabilities};

    #[test]
    fn negotiate_prefers_utf8_when_the_client_offers_it() {
        // Arrange
        let params = InitializeParams {
            capabilities: ClientCapabilities {
                general: Some(GeneralClientCapabilities {
                    position_encodings: Some(vec![
                        PositionEncodingKind::UTF16,
                        PositionEncodingKind::UTF8,
                    ]),
                    ..GeneralClientCapabilities::default()
                }),
                ..ClientCapabilities::default()
            },
            ..InitializeParams::default()
        };

        // Act
        let encoding = negotiate(&params);

        // Assert
        assert_eq!(encoding, PositionEncoding::Utf8);
    }

    #[test]
    fn negotiate_falls_back_to_utf16_when_utf8_is_absent() {
        // Arrange
        let params = InitializeParams::default();

        // Act
        let encoding = negotiate(&params);

        // Assert
        assert_eq!(encoding, PositionEncoding::Utf16);
    }

    #[test]
    fn the_advertised_capabilities_carry_the_negotiated_encoding() {
        // Arrange, Act
        let capabilities = server_capabilities(PositionEncoding::Utf8);

        // Assert
        assert_eq!(
            capabilities.position_encoding,
            Some(PositionEncodingKind::UTF8)
        );
    }
}
