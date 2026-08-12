use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode)]
    port: Located<i64>,
}

fn main() {}
