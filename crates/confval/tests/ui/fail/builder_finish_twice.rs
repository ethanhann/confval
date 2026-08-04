//! `FieldsBuilder::finish` consumes the builder, so a second call cannot
//! silently return an empty level.

use confval::format::{FieldsBuilder, Walk};
use confval::source::Located;

fn main() {
    let hostname = Located::detached("localhost".to_string());
    let builder = FieldsBuilder::new(Walk::Populated).leaf("hostname", &hostname);
    let _first = builder.finish();
    let _second = builder.finish();
}
