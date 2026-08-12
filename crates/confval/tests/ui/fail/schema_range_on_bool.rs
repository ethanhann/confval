use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = PORT)]
    flag: Located<bool>,
}

fn main() {}
