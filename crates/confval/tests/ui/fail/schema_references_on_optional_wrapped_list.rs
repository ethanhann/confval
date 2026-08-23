use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(references = upstream)]
    events: Option<Located<Vec<Located<String>>>>,
}

fn main() {}
