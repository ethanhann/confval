use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(format = Ipv4)]
    flag: Located<bool>,
}

fn main() {}
