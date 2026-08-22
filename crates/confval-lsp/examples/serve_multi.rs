//! A runnable multi binding language server over stdio, for trying routing
//! against an editor.
//!
//! Run it with `cargo run -p confval-lsp --example serve_multi`, then point an
//! LSP client at the built binary and open the documents under
//! `dev/sample_configs/multi/`. The first binding serves `gateway.cvm` by file
//! name. The second serves any file whose name starts with `device.`, through
//! the closure matcher a real host would use. Any other document is held
//! unmatched, so opening one shows the warning log and the empty answers.
//! The server routes a document when the editor opens it, so a renamed file
//! re-routes on reopen.
//!
//! The sample documents are plain HCL under the made-up `.cvm` extension, so
//! an IDE registers this server on its own file pattern beside an existing
//! `.hcl` registration for the `serve` example.

use confval::prelude::*;

use confval_lsp::{Hcl, LspError, Matcher, bind, serve_multi};

range_constraint!(PORT, i64, min: 1, max: 65535);

keyword_enum!(DeviceKind, {
    Sensor => "sensor",
    Switch => "switch",
    Camera => "camera",
});

/// The demo entrypoint, served for `gateway.cvm`.
#[derive(confval::Spec)]
struct GatewaySpec {
    /// The address the gateway binds.
    hostname: Located<String>,
    /// The TCP port the gateway listens on.
    #[confval(range = PORT)]
    port: Located<i64>,
}

/// A demo device document, served for any `device.*` file.
#[derive(confval::Spec)]
struct DeviceSpec {
    /// The device name.
    name: Located<String>,
    /// What the device is.
    #[confval(keywords = DeviceKind)]
    kind: Located<String>,
    /// The TCP port the device answers on.
    #[confval(range = PORT)]
    port: Located<i64>,
}

impl Validate for GatewaySpec {
    fn validate(&self, _report: &mut Report) {}
}

impl Validate for DeviceSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn main() -> Result<(), LspError> {
    serve_multi(vec![
        bind::<GatewaySpec, _>(Matcher::FileName("gateway.cvm".to_string()), Hcl),
        bind::<DeviceSpec, _>(
            Matcher::Fn(Box::new(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("device."))
            })),
            Hcl,
        ),
    ])
}
