use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = PORT)]
    ports: Vec<Located<String>>,
}

fn main() {}
