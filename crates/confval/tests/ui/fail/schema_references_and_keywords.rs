use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode, references = upstream)]
    mode: Located<String>,
}

fn main() {}
