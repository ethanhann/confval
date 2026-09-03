use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty(help = "a", help = "b"))]
    name: Located<String>,
}

fn main() {}
