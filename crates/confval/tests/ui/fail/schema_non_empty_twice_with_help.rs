use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty, non_empty(help = "x"))]
    name: Located<String>,
}

fn main() {}
