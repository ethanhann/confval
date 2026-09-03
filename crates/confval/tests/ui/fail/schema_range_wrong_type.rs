use confval::length_constraint;
use confval::source::Located;

length_constraint!(NAME_LEN, max: 63);

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = NAME_LEN)]
    port: Located<i64>,
}

fn main() {}
