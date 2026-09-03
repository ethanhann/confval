use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique = "x")]
    tags: Vec<Located<String>>,
}

fn main() {}
