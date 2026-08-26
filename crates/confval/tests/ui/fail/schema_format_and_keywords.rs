use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode, format = Ipv4)]
    mode: Located<String>,
}

fn main() {}
