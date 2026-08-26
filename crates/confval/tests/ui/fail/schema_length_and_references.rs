use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN, references = upstreams)]
    upstream: Located<String>,
}

fn main() {}
