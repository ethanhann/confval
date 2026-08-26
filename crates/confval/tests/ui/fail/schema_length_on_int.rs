use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN)]
    port: Located<i64>,
}

fn main() {}
