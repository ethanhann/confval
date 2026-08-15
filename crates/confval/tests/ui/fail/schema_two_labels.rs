use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(label)]
    name: Located<String>,
    #[confval(label)]
    alias: Located<String>,
}

fn main() {}
