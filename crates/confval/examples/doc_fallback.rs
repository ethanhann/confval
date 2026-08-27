//! Where a template block's comment comes from: the field's own doc when it
//! has one, and the nested spec's struct-level doc otherwise.
//!
//! Both sprocket fields below embed the same `SprocketSpec`. The primary field
//! has a doc comment, so the template renders that comment above its
//! block. The secondary field has none, so its block falls back to the struct
//! doc on `SprocketSpec`. The undocumented `max_weight` leaf renders with no
//! comment at all.
//!
//! The `templates` example covers the write path this builds on, `to_template`
//! feeding an emitter.
//!
//! Run with: cargo run -p confval --example doc_fallback --features derive,toml

use confval::prelude::*;

#[derive(confval::Spec)]
#[confval(derive_default)]
struct WidgetSpec {
    /// The primary sprocket. A field doc wins over the struct doc on
    /// `SprocketSpec`.
    #[confval(nested, default)]
    primary_sprocket: Located<SprocketSpec>,

    #[confval(default = 16)]
    max_weight: Located<i64>,

    #[confval(nested, default)]
    secondary_sprocket: Located<SprocketSpec>,
}

impl Validate for WidgetSpec {
    fn validate(&self, _report: &mut Report) {}
}

/// A sprocket's dimensions. The `secondary_sprocket` field has no doc, so its
/// block falls back to this comment.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct SprocketSpec {
    #[confval(default = 32)]
    max_height: Located<i64>,
}

impl Validate for SprocketSpec {
    fn validate(&self, _report: &mut Report) {}
}

fn main() -> Result<(), String> {
    let spec = WidgetSpec::default();

    let template =
        confval::format::toml::emit_toml(&spec.to_template()).map_err(|error| error.to_string())?;
    print!("{template}");

    Ok(())
}
