//! A runnable multi binding language server over stdio, for trying routing
//! against an editor.
//!
//! Run it with `cargo run -p confval-lsp --example serve_multi`, then point an
//! LSP client at the built binary and open the documents under
//! `dev/sample_configs/multi/`. The first binding serves `gateway.cvm` by file
//! name. The second serves any file whose name starts with `middleware.`, through
//! the closure matcher a real host would use. Any other document is held
//! unmatched, so opening one shows the warning log and the empty answers.
//! The server routes a document when the editor opens it, so a renamed file
//! re-routes on reopen.
//!
//! Pass `--workspace` to register a demo workspace validator beside the
//! per-document checks. It runs two cross-file rules: middleware names are
//! unique across middleware documents, and a middleware does not listen on the
//! port the gateway binds. For example, open the two middleware samples, change
//! one `name` to match the other, and both files report the duplicate. Or set a
//! middleware `port` to `8080`, and the middleware reports it beside a related
//! span in `gateway.cvm`.
//!
//! The sample documents are plain HCL under the made-up `.cvm` extension, so
//! an IDE registers this server on its own file pattern beside an existing
//! `.hcl` registration for the `serve` example.

use std::path::PathBuf;

use confval::prelude::*;

use confval_lsp::{
    DocumentIndex, Hcl, LspError, Matcher, WorkspaceReport, WorkspaceValidator, bind, serve_multi,
    serve_multi_with,
};

range_constraint!(PORT, i64, min: 1, max: 65535);
length_constraint!(NAME_LEN, max: 63);

keyword_enum!(MiddlewareKind, {
    Auth    => "auth",
    Cache   => "cache",
    Logging => "logging",
});

/// The demo entrypoint, served for `gateway.cvm`.
#[derive(confval::Spec)]
struct GatewaySpec {
    /// The address the gateway binds.
    #[confval(format = Ip)]
    hostname: Located<String>,
    /// The TCP port the gateway listens on.
    #[confval(range = PORT)]
    port: Located<i64>,
    /// Path to the TLS certificate file. It must be an absolute path. The
    /// example checks that the file exists in the `Validate` body.
    #[confval(format = AbsolutePath)]
    tls_cert: Option<Located<PathBuf>>,
    /// The peer addresses allowed to connect. A bare list with a default,
    /// a `unique` flag, and a format on each entry.
    #[confval(default, unique, format = Ip)]
    allow: Vec<Located<String>>,
    /// Free-form tags. A wrapped optional list with both flags, one with a
    /// help line, and no value constraint.
    #[confval(non_empty, unique(help = "Each tag may appear once."))]
    tags: Option<Located<Vec<Located<String>>>>,
}

/// A demo middleware document, served for any `middleware.*` file.
#[derive(confval::Spec)]
struct MiddlewareSpec {
    /// The middleware name.
    #[confval(non_empty(help = "A middleware name should be human-readable."), length = NAME_LEN)]
    name: Located<String>,
    /// What the middleware does.
    #[confval(keywords = MiddlewareKind)]
    kind: Located<String>,
    /// The TCP port the middleware answers on.
    #[confval(range = PORT)]
    port: Located<i64>,
    /// The middleware kinds that must run before this one. A required bare
    /// list with a `unique` flag and a keyword set.
    #[confval(unique, keywords = MiddlewareKind)]
    after: Vec<Located<String>>,
    /// The path prefixes that this middleware handles. A wrapped optional list
    /// with a `unique` flag and a format.
    #[confval(unique, format = AbsolutePath)]
    paths: Option<Located<Vec<Located<String>>>>,
    /// Operator notes. A bare list with a default and no other rule.
    #[confval(default)]
    notes: Vec<Located<String>>,
}

impl Validate for GatewaySpec {
    fn validate(&self, report: &mut Report) {
        if let Some(cert) = &self.tls_cert
            && !cert.value.exists()
        {
            report
                .error(format!("tls_cert file not found: {}", cert.value.display()))
                .at(cert.span)
                .help("Set tls_cert to a path that exists on disk.")
                .emit();
        }
    }
}

impl Validate for MiddlewareSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// The demo workspace validator. It runs two cross-file rules: middleware
/// names are unique across middleware documents, and a middleware does not
/// listen on the port the gateway binds. Each report lands in the file it
/// concerns, with a related span pointing at the other document.
struct CrossFileChecks;

impl WorkspaceValidator for CrossFileChecks {
    fn validate(&self, documents: &DocumentIndex, report: &mut WorkspaceReport) {
        let middleware: Vec<(&str, &MiddlewareSpec)> = documents
            .iter()
            .filter_map(|(uri, document)| {
                document
                    .spec()
                    .and_then(|spec| spec.downcast_ref::<MiddlewareSpec>())
                    .map(|spec| (uri, spec))
            })
            .collect();
        let gateway = documents.iter().find_map(|(_, document)| {
            document
                .spec()
                .and_then(|spec| spec.downcast_ref::<GatewaySpec>())
        });
        for (i, (uri, spec)) in middleware.iter().enumerate() {
            for (other_uri, other) in middleware.iter().skip(i + 1) {
                if spec.name.value != other.name.value {
                    continue;
                }
                report
                    .report(uri)
                    .error("duplicate middleware name across documents")
                    .at(spec.name.span)
                    .related(other.name.span, "also declared here")
                    .emit();
                report
                    .report(other_uri)
                    .error("duplicate middleware name across documents")
                    .at(other.name.span)
                    .related(spec.name.span, "also declared here")
                    .emit();
            }
        }
        if let Some(gateway) = gateway {
            for (uri, middleware) in &middleware {
                if middleware.port.value == gateway.port.value {
                    report
                        .report(uri)
                        .error("the middleware listens on the port the gateway binds")
                        .at(middleware.port.span)
                        .related(gateway.port.span, "the gateway binds this port")
                        .emit();
                }
            }
        }
    }
}

fn main() -> Result<(), LspError> {
    let workspace = std::env::args().any(|arg| arg == "--workspace");
    let bindings = vec![
        bind::<GatewaySpec, _>(Matcher::FileName("gateway.cvm".to_string()), Hcl),
        bind::<MiddlewareSpec, _>(
            Matcher::Fn(Box::new(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("middleware."))
            })),
            Hcl,
        ),
    ];
    if workspace {
        serve_multi_with(bindings, Box::new(CrossFileChecks))
    } else {
        serve_multi(bindings)
    }
}
