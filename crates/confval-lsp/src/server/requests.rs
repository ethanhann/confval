//! The positioned request handlers of [`Router`].
//!
//! Each method resolves the request against a stored document and its binding,
//! then answers through a pure handler. The transport shell in the parent
//! module dispatches to these.

use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CompletionParams, CompletionResponse, DocumentHighlight,
    DocumentHighlightParams, DocumentLink, DocumentLinkParams, DocumentSymbolParams,
    DocumentSymbolResponse, FoldingRange, FoldingRangeParams, GotoDefinitionParams, Hover,
    HoverParams, PrepareRenameResponse, ReferenceParams, RenameParams, TextDocumentPositionParams,
    Uri, WorkspaceEdit,
};

use crate::binding::Binding;
use crate::encoding::LineIndex;
use crate::frontend::{CursorContext, Recovery};
use crate::handlers;

use super::{Document, Router};

impl Router {
    /// Computes the completion response for a request.
    pub(super) fn completion(&self, params: CompletionParams) -> CompletionResponse {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, binding, index, context)) = self.resolve_at(uri, position) else {
            return CompletionResponse::Array(Vec::new());
        };
        let items = handlers::completion(
            &*binding.frontend,
            &cx(document, binding, &context),
            &index,
            self.encoding,
            self.completion_client,
        );
        CompletionResponse::Array(items)
    }

    /// Resolves the cursor of a positioned request against a stored document
    /// and its binding, so each handler does one lookup. An unmatched
    /// document resolves to `None`, and the callers answer empty.
    fn resolve_at(
        &self,
        uri: &Uri,
        position: lsp_types::Position,
    ) -> Option<(&Document, &Binding, LineIndex, CursorContext)> {
        let document = self.documents.get(uri.as_str())?;
        let binding = self.bindings.get(document.binding?)?;
        let index = LineIndex::new(&document.text);
        let offset = index.offset_of(&document.text, position, self.encoding);
        let context = binding
            .frontend
            .resolve(document.tree.as_ref(), &document.text, offset);
        Some((document, binding, index, context))
    }

    /// Computes the definition response for a request.
    pub(super) fn definition(&self, params: GotoDefinitionParams) -> Option<lsp_types::Location> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let (document, binding, index, context) = self.resolve_at(uri, position)?;
        handlers::definition(
            &binding.schema,
            &context,
            uri,
            &document.text,
            &index,
            self.encoding,
        )
    }

    /// Computes the references response for a request.
    pub(super) fn references(&self, params: ReferenceParams) -> Vec<lsp_types::Location> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, binding, index, context)) = self.resolve_at(uri, position) else {
            return Vec::new();
        };
        handlers::references(
            &binding.schema,
            &context,
            params.context.include_declaration,
            uri,
            &document.text,
            &index,
            self.encoding,
        )
    }

    /// Computes the document-symbol response for a request. A buffer that
    /// does not parse answers nothing, because the outline reads parsed
    /// spans. An unmatched document never holds a tree, so that same guard
    /// answers before the binding lookup.
    pub(super) fn document_symbols(
        &self,
        params: DocumentSymbolParams,
    ) -> Option<DocumentSymbolResponse> {
        let uri = &params.text_document.uri;
        let document = self.documents.get(uri.as_str())?;
        let tree = document.tree.as_ref()?;
        let binding = self.bindings.get(document.binding?)?;
        let index = LineIndex::new(&document.text);
        Some(handlers::document_symbols(
            &binding.schema,
            tree,
            handlers::SymbolShape {
                covers_body: binding.frontend.block_span_covers_body(),
                hierarchical: self.hierarchical,
            },
            uri,
            &document.text,
            &index,
            self.encoding,
        ))
    }

    /// Computes the document-highlight response for a request.
    pub(super) fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Vec<DocumentHighlight> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, binding, index, context)) = self.resolve_at(uri, position) else {
            return Vec::new();
        };
        handlers::document_highlight(
            &binding.schema,
            &context,
            &document.text,
            &index,
            self.encoding,
        )
    }

    /// Computes the prepare-rename response for a request.
    pub(super) fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Option<PrepareRenameResponse> {
        let (document, binding, index, context) =
            self.resolve_at(&params.text_document.uri, params.position)?;
        handlers::prepare_rename(
            &binding.schema,
            &context,
            &document.text,
            &index,
            self.encoding,
        )
    }

    /// Computes the rename response for a request. The transport answers a
    /// refused name as an error.
    pub(super) fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>, String> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, binding, index, context)) = self.resolve_at(uri, position) else {
            return Ok(None);
        };
        handlers::rename(
            &binding.schema,
            &context,
            uri,
            &document.text,
            &index,
            self.encoding,
            &params.new_name,
        )
    }

    /// Computes the folding ranges for a request. A buffer that does not
    /// parse answers empty, because the folds read parsed spans.
    pub(super) fn folding_ranges(&self, params: FoldingRangeParams) -> Vec<FoldingRange> {
        let uri = &params.text_document.uri;
        let Some(document) = self.documents.get(uri.as_str()) else {
            return Vec::new();
        };
        let Some(tree) = document.tree.as_ref() else {
            return Vec::new();
        };
        let Some(binding) = document.binding.and_then(|i| self.bindings.get(i)) else {
            return Vec::new();
        };
        let index = LineIndex::new(&document.text);
        let brace_format = matches!(
            binding.frontend.recovery(),
            Recovery::Braces | Recovery::Object
        );
        handlers::folding_ranges(
            &binding.schema,
            tree,
            &document.text,
            binding.frontend.block_span_covers_body(),
            brace_format,
            &index,
            self.encoding,
        )
    }

    /// Collects document links for path-typed fields in the parsed tree.
    pub(super) fn document_links(&self, params: DocumentLinkParams) -> Vec<DocumentLink> {
        let uri = &params.text_document.uri;
        let Some(document) = self.documents.get(uri.as_str()) else {
            // The editor sent a request for a document the server has not opened.
            return Vec::new();
        };
        let Some(tree) = document.tree.as_ref() else {
            // The document has a syntax error and produced no parsed tree.
            return Vec::new();
        };
        let Some(binding) = document.binding.and_then(|i| self.bindings.get(i)) else {
            // The document matched no binding at open, so it has no schema.
            return Vec::new();
        };
        let index = LineIndex::new(&document.text);
        handlers::document_links(
            &binding.schema,
            tree,
            uri,
            &document.text,
            &index,
            self.encoding,
        )
    }

    /// Computes the code-action response for a request, resolved at the
    /// request range's start.
    pub(super) fn code_action(&self, params: CodeActionParams) -> Vec<CodeActionOrCommand> {
        let uri = &params.text_document.uri;
        let Some((document, binding, index, context)) = self.resolve_at(uri, params.range.start)
        else {
            return Vec::new();
        };
        handlers::code_action(
            &*binding.frontend,
            &cx(document, binding, &context),
            &params.context.diagnostics,
            params.context.only.as_deref(),
            uri,
            &index,
            self.encoding,
        )
    }

    /// Computes the hover response for a request.
    pub(super) fn hover(&self, params: HoverParams) -> Option<Hover> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let (document, binding, index, context) = self.resolve_at(uri, position)?;
        handlers::hover(&cx(document, binding, &context), &index, self.encoding)
    }
}

/// The handler context for a resolved document and its binding.
fn cx<'a>(
    document: &'a Document,
    binding: &'a Binding,
    context: &'a CursorContext,
) -> handlers::Cx<'a> {
    handlers::Cx {
        schema: &binding.schema,
        fields: document.tree.as_ref(),
        ctx: context,
        text: &document.text,
    }
}
