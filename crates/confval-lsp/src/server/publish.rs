//! The diagnostics publishing flow of [`Router`].
//!
//! Each publish sends one document's merged diagnostic set. Without a
//! workspace validator, the set is the document's own per-document
//! diagnostics. With one, the pass's issues attributed to that document are
//! merged in. The validate pass stores its lowered spec on the entry. The
//! workspace pass reads that instance. Every open matched document publishes
//! on every notification, so a diagnostic that disappears from the report
//! clears everywhere.

use std::any::Any;

use lsp_server::{Connection, Message, Notification};
use lsp_types::notification::{Notification as _, PublishDiagnostics};
use lsp_types::{Diagnostic, PublishDiagnosticsParams, Uri};

use crate::handlers;
use crate::workspace::WorkspaceReport;

use super::{LspError, Router};

impl Router {
    /// Computes and stores the lowered spec of one document, answering its
    /// per-document diagnostics. A document the index does not hold answers
    /// empty. The workspace pass reads the stored spec, so both passes
    /// validate the same instance.
    fn document_diagnostics(&mut self, key: &str) -> Vec<Diagnostic> {
        let Some(computed) = self.compute_diagnostics(key) else {
            return Vec::new();
        };
        if let Some(entry) = self.index.get_mut(key) {
            entry.set_spec(computed.spec);
        }
        computed.diagnostics
    }

    /// The diagnostics and the lowered spec of one document, or `None` when
    /// the index does not hold it or no binding matched.
    fn compute_diagnostics(&self, key: &str) -> Option<Computed> {
        let entry = self.index.get(key)?;
        let binding = self.bindings.get(entry.binding()?)?;
        let (diagnostics, spec) = handlers::diagnostics(
            binding.validator,
            &binding.schema,
            entry.fields(),
            entry.parse_report(),
            entry.uri(),
            entry.text(),
            self.encoding,
        );
        Some(Computed { diagnostics, spec })
    }

    /// Runs the workspace pass into a fresh report, or `None` when no
    /// validator is registered. The returned report is owned. The caller can
    /// update documents while publishing it.
    fn workspace_report(&mut self) -> Option<WorkspaceReport> {
        let validator = self.workspace.as_deref()?;
        let mut report = WorkspaceReport::default();
        validator.validate(&self.index, &mut report);
        Some(report)
    }

    /// Publishes after an open or change. Without a validator, the document's
    /// own diagnostics publish, and an unmatched document publishes nothing.
    /// With one, the pass runs once. Every open matched document is published
    /// with its full merged set, so a diagnostic that disappears from the
    /// report clears everywhere.
    pub(super) fn publish_after_update(
        &mut self,
        connection: &Connection,
        uri: &Uri,
    ) -> Result<(), LspError> {
        let own = self.document_diagnostics(uri.as_str());
        match self.workspace_report() {
            None => {
                if self.index.get(uri.as_str()).is_some() {
                    self.send_diagnostics(connection, uri, own)?;
                }
                Ok(())
            }
            Some(report) => {
                let changed = uri.as_str().to_string();
                for (key, publish_uri) in self.publish_targets() {
                    let mut merged = if key == changed {
                        own.clone()
                    } else {
                        self.document_diagnostics(&key)
                    };
                    merged.extend(handlers::workspace_diagnostics(
                        &report,
                        &self.index,
                        &key,
                        self.encoding,
                    ));
                    self.send_diagnostics(connection, &publish_uri, merged)?;
                }
                Ok(())
            }
        }
    }

    /// Publishes the merged set of every open matched document after a close.
    /// The closed document received its clear above. A no-validator server
    /// publishes nothing here.
    pub(super) fn publish_workspace(&mut self, connection: &Connection) -> Result<(), LspError> {
        let Some(report) = self.workspace_report() else {
            return Ok(());
        };
        for (key, publish_uri) in self.publish_targets() {
            let mut merged = self.document_diagnostics(&key);
            merged.extend(handlers::workspace_diagnostics(
                &report,
                &self.index,
                &key,
                self.encoding,
            ));
            self.send_diagnostics(connection, &publish_uri, merged)?;
        }
        Ok(())
    }

    /// Every open matched document as its index key and URI, in URI order, so
    /// the merged publish is deterministic.
    fn publish_targets(&self) -> Vec<(String, Uri)> {
        self.index
            .iter()
            .map(|(key, entry)| (key.to_string(), entry.uri().clone()))
            .collect()
    }

    /// Sends one publish-diagnostics notification.
    pub(super) fn send_diagnostics(
        &self,
        connection: &Connection,
        uri: &Uri,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<(), LspError> {
        connection
            .sender
            .send(Message::Notification(Notification::new(
                PublishDiagnostics::METHOD.to_string(),
                PublishDiagnosticsParams {
                    uri: uri.clone(),
                    diagnostics,
                    version: None,
                },
            )))?;
        Ok(())
    }
}

/// The diagnostics and the lowered spec one document's publish produces.
struct Computed {
    diagnostics: Vec<Diagnostic>,
    spec: Option<Box<dyn Any>>,
}
