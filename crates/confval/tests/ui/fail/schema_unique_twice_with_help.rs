use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique, unique(help = "x"))]
    tags: Vec<Located<String>>,
}

fn main() {}
