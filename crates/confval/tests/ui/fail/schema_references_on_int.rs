use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(references = upstream)]
    port: Located<i64>,
}

fn main() {}
