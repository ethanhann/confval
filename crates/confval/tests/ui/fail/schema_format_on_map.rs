use confval::source::Located;
use std::collections::BTreeMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map, format = Ipv4)]
    headers: BTreeMap<String, Located<String>>,
}

fn main() {}
