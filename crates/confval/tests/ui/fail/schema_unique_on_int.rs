use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique)]
    port: Located<i64>,
}

fn main() {}
