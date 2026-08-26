use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN)]
    ratio: Located<f64>,
}

fn main() {}
