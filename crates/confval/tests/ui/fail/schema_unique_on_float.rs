use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique)]
    ratio: Located<f64>,
}

fn main() {}
