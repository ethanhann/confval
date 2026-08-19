use confval::source::Located;

#[derive(confval::Spec)]
struct Node {
    name: Located<String>,
    #[confval(nested)]
    children: Vec<Located<Node>>,
}

fn main() {}
