use confval::range_constraint;
use confval::source::Located;

range_constraint!(PORT, i64, min: 1, max: 65535);

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = PORT)]
    name: Located<String>,
}

fn main() {}
