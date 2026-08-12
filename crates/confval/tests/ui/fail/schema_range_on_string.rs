use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = PORT)]
    name: Located<String>,
}

fn main() {}
