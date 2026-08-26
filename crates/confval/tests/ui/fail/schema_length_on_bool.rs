use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN)]
    flag: Located<bool>,
}

fn main() {}
