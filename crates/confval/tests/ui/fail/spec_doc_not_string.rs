use confval::source::Located;

// `#[confval(doc = ...)]` takes a string literal, the comment to render above
// the field. A non-string value is a compile error.
#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(doc = 42)]
    port: Located<i64>,
}

fn main() {}
