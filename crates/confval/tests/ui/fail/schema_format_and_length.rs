use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN, format = Ipv4)]
    bind: Located<String>,
}

fn main() {}
