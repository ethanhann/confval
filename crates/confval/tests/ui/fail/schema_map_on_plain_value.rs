use std::collections::BTreeMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map)]
    headers: BTreeMap<String, String>,
}

fn main() {}
