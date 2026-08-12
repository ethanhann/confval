use confval::source::Located;

#[derive(confval::Spec)]
struct Child {
    value: Located<i64>,
}

#[derive(confval::Spec)]
struct Cfg {
    #[confval(nested, range = PORT)]
    child: Located<Child>,
}

fn main() {}
