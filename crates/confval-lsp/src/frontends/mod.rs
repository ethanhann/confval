//! The five format frontends: HCL, TOML, KDL, JSON, and YAML.
//!
//! Each binds its `confval` parse function, its recovery strategy, and its
//! insert text. Everything else, parsing and resolving a cursor, is the shared
//! default on [`Frontend`], except the YAML frontend, whose indentation recovery
//! reads the raw text in both parse states.

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

/// Whether a field is written as a block rather than an attribute.
fn is_block(field: &SchemaField) -> bool {
    matches!(field.ty, SchemaType::Block { .. })
}
