use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique)]
    name: Located<String>,
}

fn main() {}
