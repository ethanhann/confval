use confval::source::Located;
use std::collections::BTreeMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map, default = 5)]
    headers: BTreeMap<String, Located<String>>,
}

fn main() {}
