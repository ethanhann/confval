use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique(hint = "x"))]
    tags: Vec<Located<String>>,
}

fn main() {}
