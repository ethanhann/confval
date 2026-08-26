use confval::source::Located;
use std::path::PathBuf;

#[derive(confval::Spec)]
struct Cfg {
    #[confval(length = NAME_LEN)]
    root: Located<PathBuf>,
}

fn main() {}
