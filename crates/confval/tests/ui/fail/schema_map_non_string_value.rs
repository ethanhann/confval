use confval::source::Located;
use std::collections::BTreeMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map)]
    headers: BTreeMap<String, Located<i64>>,
}

fn main() {}
