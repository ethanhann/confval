use confval::source::Located;
use std::path::PathBuf;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(unique)]
    root: Located<PathBuf>,
}

fn main() {}
