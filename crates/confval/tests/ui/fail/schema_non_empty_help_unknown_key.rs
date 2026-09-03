use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty(hint = "x"))]
    name: Located<String>,
}

fn main() {}
