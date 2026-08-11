use confval::source::Located;
use std::collections::BTreeMap;

#[derive(confval::Spec)]
#[confval(derive_default)]
struct Cfg {
    #[confval(map)]
    headers: BTreeMap<String, Located<String>>,
}

fn main() {}
