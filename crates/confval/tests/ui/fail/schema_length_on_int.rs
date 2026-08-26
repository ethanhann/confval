use confval::prelude::*;

length_constraint!(NAME_LEN, min: 1, max: 63);

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN)]
    port: Located<i64>,
}

fn main() {}
