use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty)]
    ratio: Located<f64>,
}

fn main() {}
