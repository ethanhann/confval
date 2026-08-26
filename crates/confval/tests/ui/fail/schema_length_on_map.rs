use confval::source::Located;
use std::collections::BTreeMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map, length = NAME_LEN)]
    headers: BTreeMap<String, Located<String>>,
}

fn main() {}
