use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty(help = 1))]
    name: Located<String>,
}

fn main() {}
