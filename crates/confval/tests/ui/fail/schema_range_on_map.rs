use confval::source::Located;
use std::collections::BTreeMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map, range = PORT)]
    headers: BTreeMap<String, Located<String>>,
}

fn main() {}
