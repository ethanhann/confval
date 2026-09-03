//! `#[confval(non_empty)]` pass test over every legal carrier.
//!
//! This pins the two leaf shapes and the two list shapes as legal, in the
//! bare form and in the `help = "..."` form, and pins the flag beside a value
//! constraint, so each form stays compilable rather than resting on the
//! fail cases alone.

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
    #[confval(label)]
    name: Located<String>,
    #[confval(non_empty, keywords = LimitMode)]
    mode: Located<String>,
    #[confval(non_empty)]
    region: Option<Located<String>>,
    #[confval(non_empty)]
    tags: Vec<Located<String>>,
    #[confval(non_empty)]
    events: Option<Located<Vec<Located<String>>>>,
    #[confval(non_empty(help = "Provide a socket path."))]
    sock: Located<String>,
    #[confval(non_empty(help = "Provide a zone or omit it."))]
    zone: Option<Located<String>>,
    #[confval(non_empty(help = "List at least one hook."))]
    hooks: Vec<Located<String>>,
    #[confval(non_empty(help = "List at least one phase."))]
    phases: Option<Located<Vec<Located<String>>>>,
    #[confval(non_empty(help = "Provide a limit mode."), keywords = LimitMode)]
    limit: Located<String>,
}

impl Validate for Cfg {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
