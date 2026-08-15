use confval::source::Located;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(label, default = "fallback".to_string())]
    name: Located<String>,
}

fn main() {}
