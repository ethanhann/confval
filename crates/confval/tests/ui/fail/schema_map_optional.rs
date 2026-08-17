use confval::source::Located;
use std::collections::BTreeMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map)]
    headers: Option<BTreeMap<String, Located<String>>>,
}

fn main() {}
