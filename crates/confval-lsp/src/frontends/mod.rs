//! The five format frontends: HCL, TOML, KDL, JSON, and YAML.
//!
//! Each binds its `confval` parse function, its recovery strategy, and its
//! insert text. Everything else, building the tree and resolving a cursor, is the shared
//! default on [`Frontend`]. The YAML frontend uses that same default `resolve`,
//! which routes its indentation recovery to the YAML reader in both parse states.

mod hcl;
mod json;
mod kdl;
mod toml;
mod yaml;

pub use self::hcl::Hcl;
pub use self::json::Json;
pub use self::kdl::Kdl;
pub use self::toml::Toml;
pub use self::yaml::Yaml;

use confval::schema::{SchemaField, SchemaType};

use crate::Frontend;

/// Whether a field is written as a block rather than an attribute.
fn is_block(field: &SchemaField) -> bool {
    matches!(field.ty, SchemaType::Block { .. })
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
    let literal = snippet_escape(&frontend.default_literal(leaf, text));
    format!("${{1:{literal}}}")
}

/// Escapes the snippet metacharacters, so user text passes through a
/// placeholder verbatim.
fn snippet_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '$' | '}' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}
