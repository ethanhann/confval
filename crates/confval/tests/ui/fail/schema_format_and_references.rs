use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(format = Ipv4, references = upstreams)]
    upstream: Located<String>,
}

fn main() {}
