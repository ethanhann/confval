use confval::source::Located;

// A field-level `derive_default` needs the struct-level flag that asks for the
// generated `Default` impl.
#[derive(confval::Spec)]
struct Spec {
    #[confval(derive_default = false)]
    enable: Located<bool>,
}

fn main() {}
