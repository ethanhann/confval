use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(range = PORT, format = Ipv4)]
    port: Located<i64>,
}

fn main() {}
