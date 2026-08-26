//! `#[confval(unique)]` pass test over every legal carrier.
//!
//! This pins the bare and the wrapped string list as legal carriers of
//! `#[confval(unique)]`, and pins the flag beside `default`, beside
//! `non_empty`, beside `keywords`, and beside `format`. The fail cases alone
//! do not prove the legal forms compile.

use confval::diagnostic::Report;
use confval::keyword_enum;
use confval::pipeline::Validate;
use confval::source::Located;
use confval::Ip;

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
});

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique)]
    tags: Vec<Located<String>>,
    #[confval(unique)]
    labels: Option<Located<Vec<Located<String>>>>,
    #[confval(default, unique)]
    extra: Vec<Located<String>>,
    #[confval(non_empty, unique)]
    names: Vec<Located<String>>,
    #[confval(unique, keywords = LimitMode)]
    modes: Vec<Located<String>>,
    #[confval(unique, format = Ip)]
    peers: Vec<Located<String>>,
}

impl Validate for Cfg {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
