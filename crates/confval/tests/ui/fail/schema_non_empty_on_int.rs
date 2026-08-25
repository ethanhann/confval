use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty)]
    count: Located<i64>,
}

fn main() {}
