use confval::source::Located;

#[derive(confval::Spec)]
struct Child {
    value: Located<i64>,
}

#[derive(confval::Spec)]
struct Cfg {
    #[confval(nested, keywords = LimitMode)]
    child: Located<Child>,
}

fn main() {}
