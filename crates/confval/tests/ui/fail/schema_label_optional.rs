use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(label)]
    name: Option<Located<String>>,
}

fn main() {}
