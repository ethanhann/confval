use confval::source::Located;

struct Palette;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = Palette)]
    mode: Located<String>,
}

fn main() {}
