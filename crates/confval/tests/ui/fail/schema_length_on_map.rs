use confval::prelude::*;
use std::collections::BTreeMap;

length_constraint!(NAME_LEN, min: 1, max: 63);

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map, length = NAME_LEN)]
    headers: BTreeMap<String, Located<String>>,
}

fn main() {}
