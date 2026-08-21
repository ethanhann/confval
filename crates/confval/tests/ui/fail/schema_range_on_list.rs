use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = PORT)]
    items: Vec<Located<String>>,
}

fn main() {}
