use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN)]
    events: Option<Located<Vec<Located<String>>>>,
}

fn main() {}
