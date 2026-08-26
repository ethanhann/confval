use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN)]
    tags: Vec<Located<String>>,
}

fn main() {}
