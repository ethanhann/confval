//! `#[confval(length = ...)]` pass test over every legal carrier.
//!
//! This pins the required and the optional `String` leaf as legal, and pins
//! the constraint beside `default`, beside `non_empty`, and beside `label`,
//! so the spelling stays compilable rather than resting on the fail cases
//! alone.

use confval::diagnostic::Report;
use confval::length_constraint;
use confval::pipeline::Validate;
use confval::source::Located;

length_constraint!(NAME_LEN, min: 1, max: 63);

#[derive(confval::Spec)]
struct Cfg {
    #[confval(label, length = NAME_LEN)]
    name: Located<String>,
    #[confval(non_empty, length = NAME_LEN)]
    hostname: Located<String>,
    #[confval(default = "us".to_string(), length = NAME_LEN)]
    region: Located<String>,
    #[confval(length = NAME_LEN)]
    zone: Option<Located<String>>,
}

impl Validate for Cfg {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
