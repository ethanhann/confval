//! The schema-generic language server core for confval configuration files.
//!
//! confval rejects an unknown field at process startup, so a mistake in a
//! handwritten configuration surfaces as a hard failure rather than a silently
//! ignored key. This crate answers the editor's questions before the
//! program runs: which fields are legal here, what each one holds, which values a
//! closed-set field accepts, and where the file is wrong.
//!
//! The core is three layers. The pure [`handlers`] are the center, each a
//! function of the document, the schema, and a resolved cursor context. The
//! [`Frontend`] trait is the one format-dependent boundary, with an implementation
//! for each format confval parses ([`Hcl`], [`Toml`], [`Kdl`], [`Json`], [`Yaml`]). The transport
//! shell wires the handlers and a document store into a runnable server.
//!
//! The core is spec erased. [`bind`] captures the traits the derive emits,
//! `FromFields`, `Validate`, `ValidateNested`, and `ToSchema`, into a
//! [`Binding`], and a [`Router`] serves one binding per document shape, so
//! one process serves every document of a multi document configuration.
//! [`serve`] binds one root spec and one frontend for the single shape case.

mod binding;
mod capabilities;
mod encoding;
mod frontend;
mod frontends;
mod resolve;
mod scan;
mod server;
mod snippet;
mod walk;

pub mod handlers;

pub use binding::{Binding, Matcher, Validator, bind};
pub use encoding::{LineIndex, PositionEncoding};
pub use frontend::{
    Absorb, CursorContext, Frontend, Insert, PositionKind, Recovery, ValueSeparator,
};
#[cfg(feature = "hcl")]
pub use frontends::Hcl;
#[cfg(feature = "json")]
pub use frontends::Json;
#[cfg(feature = "kdl")]
pub use frontends::Kdl;
#[cfg(feature = "toml")]
pub use frontends::Toml;
#[cfg(feature = "yaml")]
pub use frontends::Yaml;
pub use server::{LspError, Router, Server, serve, serve_multi};
