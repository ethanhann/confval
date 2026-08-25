//! `#[confval(non_empty)]` pass test over every legal carrier.
//!
//! This pins the two leaf shapes and the two list shapes as legal, and pins
//! the flag beside `label` and beside a value constraint, so the spelling
//! stays compilable rather than resting on the fail cases alone.

use confval::diagnostic::Report;
use confval::keyword_enum;
use confval::pipeline::Validate;
use confval::source::Located;

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

#[derive(confval::Spec)]
struct Cfg {
    #[confval(label, non_empty)]
    name: Located<String>,
    #[confval(non_empty, keywords = LimitMode)]
    mode: Located<String>,
    #[confval(non_empty)]
    region: Option<Located<String>>,
    #[confval(non_empty)]
    tags: Vec<Located<String>>,
    #[confval(non_empty)]
    events: Option<Located<Vec<Located<String>>>>,
}

impl Validate for Cfg {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
