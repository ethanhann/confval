use confval::source::Located;
use std::path::PathBuf;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(non_empty)]
    root: Located<PathBuf>,
}

fn main() {}
