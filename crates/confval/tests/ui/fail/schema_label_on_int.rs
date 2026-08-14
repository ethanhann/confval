use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(label)]
    port: Located<i64>,
}

fn main() {}
