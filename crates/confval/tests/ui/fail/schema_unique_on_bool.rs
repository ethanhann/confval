use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique)]
    flag: Located<bool>,
}

fn main() {}
