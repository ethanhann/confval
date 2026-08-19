use confval::source::Located;
use std::collections::HashMap;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map)]
    headers: HashMap<String, Located<String>>,
}

fn main() {}
