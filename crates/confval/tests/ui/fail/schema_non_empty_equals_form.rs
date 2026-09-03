use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty = "x")]
    name: Located<String>,
}

fn main() {}
