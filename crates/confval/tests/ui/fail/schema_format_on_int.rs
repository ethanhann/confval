use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(format = Ipv4)]
    port: Located<i64>,
}

fn main() {}
