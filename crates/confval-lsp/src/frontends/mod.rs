//! The five format frontends: HCL, TOML, KDL, JSON, and YAML.
//!
//! Each binds its `confval` parse function, its recovery strategy, and its
//! insert text. Everything else, building the tree and resolving a cursor, is the shared
//! default on [`Frontend`]. The YAML frontend uses that same default `resolve`,
//! which routes its indentation recovery to the YAML reader in both parse states.

#[cfg(feature = "hcl")]
mod hcl;
#[cfg(feature = "json")]
mod json;
#[cfg(feature = "kdl")]
mod kdl;
#[cfg(feature = "toml")]
mod toml;
#[cfg(feature = "yaml")]
mod yaml;

#[cfg(feature = "hcl")]
pub use self::hcl::Hcl;
#[cfg(feature = "json")]
pub use self::json::Json;
#[cfg(feature = "kdl")]
pub use self::kdl::Kdl;
#[cfg(feature = "toml")]
pub use self::toml::Toml;
#[cfg(feature = "yaml")]
pub use self::yaml::Yaml;

use confval::schema::{SchemaField, SchemaType};

use crate::Frontend;

/// Whether a field is written as a block rather than an attribute.
fn is_block(field: &SchemaField) -> bool {
    matches!(field.ty, SchemaType::Block { .. })
}

/// The insert for a scalar attribute: a snippet when a default placeholder is
/// present, a literal otherwise.
fn scalar_insert(text: String, placeholder: &str) -> crate::frontend::Insert {
    if placeholder.is_empty() {
        crate::frontend::Insert::plain(text)
    } else {
        crate::frontend::Insert::snippet(text)
    }
}

/// The pre-filled value for a defaulted scalar: the format literal inside a
/// selected snippet placeholder, or the empty string when the field carries no
/// rendered default. The literal is snippet-escaped, so a `$`, `}`, or `\` in
/// a default cannot corrupt the snippet grammar.
fn value_placeholder<F: Frontend + ?Sized>(frontend: &F, field: &SchemaField) -> String {
    let SchemaType::Scalar { leaf, .. } = &field.ty else {
        return String::new();
    };
    let Some(text) = &field.default_text else {
        return String::new();
    };
    let literal = crate::snippet::escape(&frontend.default_literal(leaf, text));
    format!("${{1:{literal}}}")
}
