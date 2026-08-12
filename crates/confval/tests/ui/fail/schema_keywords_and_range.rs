use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode, range = PORT)]
    mode: Located<String>,
}

fn main() {}
