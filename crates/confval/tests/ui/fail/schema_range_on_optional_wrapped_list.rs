use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = PORT)]
    events: Option<Located<Vec<Located<String>>>>,
}

fn main() {}
