use confval::source::Located;

// The field-level form states the value explicitly, so the bare flag is refused.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(derive_default)]
    enable: Located<bool>,
}

fn main() {}
