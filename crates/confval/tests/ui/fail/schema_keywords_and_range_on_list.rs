use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode, range = PORT)]
    modes: Vec<Located<String>>,
}

fn main() {}
