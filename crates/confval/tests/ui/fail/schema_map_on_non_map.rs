use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(map)]
    headers: Located<String>,
}

fn main() {}
