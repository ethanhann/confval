use confval::prelude::*;

length_constraint!(NAME_LEN, min: 1, max: 63);
range_constraint!(PORT, i64, min: 1, max: 65535);

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = PORT, length = NAME_LEN)]
    port: Located<i64>,
}

fn main() {}
