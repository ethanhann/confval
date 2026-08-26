use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique)]
    name: Option<Located<String>>,
}

fn main() {}
