use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty(help = "  "))]
    name: Located<String>,
}

fn main() {}
