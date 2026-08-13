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
//! The core is generic over the root spec `S`, needing only the traits the
//! derive emits: `FromFields`, `Validate`, `ValidateNested`, and `ToSchema`.

mod capabilities;
mod encoding;
mod frontend;
mod frontends;
mod resolve;
mod server;
mod text_scan;
mod walk;
mod yaml_scan;

pub mod handlers;

pub use encoding::{LineIndex, PositionEncoding};
pub use frontend::{CursorContext, Frontend, PositionKind, Recovery, ValueSeparator};
pub use frontends::{Hcl, Json, Kdl, Toml, Yaml};
pub use server::{Server, serve};
