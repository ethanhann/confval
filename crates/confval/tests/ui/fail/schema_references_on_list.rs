use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(references = upstream)]
    items: Vec<Located<String>>,
}

fn main() {}
