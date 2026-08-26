use confval::source::Located;
use std::path::PathBuf;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(format = AbsolutePath)]
    root: Located<PathBuf>,
}

fn main() {}
