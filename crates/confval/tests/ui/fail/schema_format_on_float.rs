use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(format = Ipv4)]
    ratio: Located<f64>,
}

fn main() {}
