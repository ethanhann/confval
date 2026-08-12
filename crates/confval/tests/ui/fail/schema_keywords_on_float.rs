use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode)]
    ratio: Located<f64>,
}

fn main() {}
