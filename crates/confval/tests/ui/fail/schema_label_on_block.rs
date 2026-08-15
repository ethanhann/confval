use confval::source::Located;

#[derive(confval::Spec)]
struct Inner {
    host: Located<String>,
}

#[derive(confval::Spec)]
struct Cfg {
    #[confval(nested, label)]
    inner: Located<Inner>,
}

fn main() {}
