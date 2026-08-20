//! `#[confval(keywords = ...)]` on a string list pass test.
//!
//! This pins the two list shapes as legal carriers of a keyword set, so the
//! spelling stays compilable rather than resting on the fail cases alone.

use confval::diagnostic::Report;
use confval::pipeline::Validate;
use confval::source::Located;
use confval::keyword_enum;

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

#[derive(confval::Spec)]
struct Cfg {
    #[confval(default, keywords = LimitMode)]
    modes: Vec<Located<String>>,
    #[confval(keywords = LimitMode)]
    events: Option<Located<Vec<Located<String>>>>,
}

impl Validate for Cfg {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
