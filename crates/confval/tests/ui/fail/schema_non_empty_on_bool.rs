use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty)]
    flag: Located<bool>,
}

fn main() {}
