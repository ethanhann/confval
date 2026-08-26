//! `#[confval(format = ...)]` pass test over every legal carrier.
//!
//! This pins the required and the optional `String` leaf and both list
//! shapes as legal carriers of `#[confval(format = ...)]`, and pins the
//! attribute beside `default`, beside `non_empty`, and beside `label`. The
//! fail cases alone do not prove the legal forms compile.
//!
//! The `non_empty` pairing compiles although the guide advises against it
//! for a built-in format, because a consumer format may accept the empty
//! string and the derive cannot tell the two apart.

use confval::diagnostic::Report;
use confval::pipeline::Validate;
use confval::source::Located;
use confval::{AbsolutePath, Ip, Ipv4};

#[derive(confval::Spec)]
struct Cfg {
    #[confval(label, format = Ipv4)]
    name: Located<String>,
    #[confval(non_empty, format = Ip)]
    bind: Located<String>,
    #[confval(default = "127.0.0.1".to_string(), format = Ipv4)]
    admin: Located<String>,
    #[confval(format = Ip)]
    peer: Option<Located<String>>,
    #[confval(format = Ip)]
    allow: Vec<Located<String>>,
    #[confval(format = AbsolutePath)]
    roots: Option<Located<Vec<Located<String>>>>,
}

impl Validate for Cfg {
    fn validate(&self, _report: &mut Report) {}
}

fn main() {}
