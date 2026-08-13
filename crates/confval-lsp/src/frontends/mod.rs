//! The three block-structured frontends: HCL, TOML, and KDL.
//!
//! Each binds its `confval` parse function and its insert spelling. Everything
//! else, parsing and resolving a cursor, is the shared default on [`Frontend`].

mod hcl;
mod kdl;
mod toml;

pub use self::hcl::Hcl;
pub use self::kdl::Kdl;
pub use self::toml::Toml;

use confval::schema::{SchemaField, SchemaType};

/// Whether a field is written as a block rather than an attribute.
fn is_block(field: &SchemaField) -> bool {
    matches!(field.ty, SchemaType::Block { .. })
}
