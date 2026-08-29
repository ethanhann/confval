//! The initialize handshake helpers: encoding negotiation and the advertised
//! server capabilities.

use lsp_types::{
    CodeActionProviderCapability, CompletionOptions, FoldingRangeProviderCapability,
    HoverProviderCapability, InitializeParams, OneOf, PositionEncodingKind, RenameOptions,
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

/// Reads the client's completion switches once at initialization.
pub(crate) fn completion_support(params: &InitializeParams) -> crate::handlers::ClientSupport {
    let item = params
        .capabilities
        .text_document
        .as_ref()
        .and_then(|document| document.completion.as_ref())
        .and_then(|completion| completion.completion_item.as_ref());
    crate::handlers::ClientSupport {
        snippets: item.and_then(|item| item.snippet_support).unwrap_or(false),
        preselect: item
            .and_then(|item| item.preselect_support)
            .unwrap_or(false),
    }
}

/// Whether the client renders a hierarchical document-symbol tree. A client
/// without it receives the flat form.
pub(crate) fn supports_hierarchical_symbols(params: &InitializeParams) -> bool {
    params
        .capabilities
        .text_document
        .as_ref()
        .and_then(|document| document.document_symbol.as_ref())
        .and_then(|symbols| symbols.hierarchical_document_symbol_support)
        .unwrap_or(false)
}

/// The server's advertised capabilities.
pub(crate) fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding_kind(encoding)),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions::default()),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        document_link_provider: Some(lsp_types::DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        document_highlight_provider: Some(OneOf::Left(true)),
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
    use lsp_types::{
        ClientCapabilities, CompletionClientCapabilities, CompletionItemCapability,
        GeneralClientCapabilities, TextDocumentClientCapabilities,
    };

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
    fn snippet_support_is_read_from_the_client_capabilities() {
        // Arrange
        let with_snippets = InitializeParams {
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    completion: Some(CompletionClientCapabilities {
                        completion_item: Some(CompletionItemCapability {
                            snippet_support: Some(true),
                            ..CompletionItemCapability::default()
                        }),
                        ..CompletionClientCapabilities::default()
                    }),
                    ..TextDocumentClientCapabilities::default()
                }),
                ..ClientCapabilities::default()
            },
            ..InitializeParams::default()
        };

        // Act, Assert
        assert!(completion_support(&with_snippets).snippets);
        assert!(!completion_support(&InitializeParams::default()).snippets);
        assert!(!completion_support(&InitializeParams::default()).preselect);
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

    #[test]
    fn the_advertised_capabilities_populate_every_provider_field() {
        // Arrange, Act
        let capabilities = server_capabilities(PositionEncoding::Utf16);

        // Assert
        assert_eq!(
            capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
        );
        assert_eq!(
            capabilities.completion_provider,
            Some(CompletionOptions::default())
        );
        assert_eq!(
            capabilities.hover_provider,
            Some(HoverProviderCapability::Simple(true))
        );
        assert!(
            capabilities.document_link_provider.is_some(),
            "document link provider is advertised"
        );
        assert_eq!(
            capabilities.folding_range_provider,
            Some(FoldingRangeProviderCapability::Simple(true)),
            "the folding range provider is advertised"
        );
        assert_eq!(
            capabilities.rename_provider,
            Some(OneOf::Right(RenameOptions {
                prepare_provider: Some(true),
                work_done_progress_options: Default::default(),
            })),
            "the rename provider is advertised with prepare support"
        );
        assert_eq!(
            capabilities.document_highlight_provider,
            Some(OneOf::Left(true)),
            "the document highlight provider is advertised"
        );
    }

    #[test]
    fn the_hierarchical_symbol_gate_reads_the_client_capability() {
        // Arrange
        let mut with_hierarchy = InitializeParams::default();
        with_hierarchy.capabilities.text_document =
            Some(lsp_types::TextDocumentClientCapabilities {
                document_symbol: Some(lsp_types::DocumentSymbolClientCapabilities {
                    hierarchical_document_symbol_support: Some(true),
                    ..lsp_types::DocumentSymbolClientCapabilities::default()
                }),
                ..lsp_types::TextDocumentClientCapabilities::default()
            });

        // Act
        let with_support = supports_hierarchical_symbols(&with_hierarchy);
        let without = supports_hierarchical_symbols(&InitializeParams::default());

        // Assert
        assert!(with_support);
        assert!(!without);
    }
}
