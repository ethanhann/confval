use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode, length = NAME_LEN)]
    mode: Located<String>,
}

fn main() {}
