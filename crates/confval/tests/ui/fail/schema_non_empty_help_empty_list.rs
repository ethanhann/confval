use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty())]
    name: Located<String>,
}

fn main() {}
