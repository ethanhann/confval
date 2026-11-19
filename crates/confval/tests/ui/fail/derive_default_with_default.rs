use confval::source::Located;

// `default` already supplies the generated value and makes the field optional,
// so the two attributes cannot stand on one field.
#[derive(confval::Spec)]
#[confval(derive_default)]
struct Spec {
    #[confval(default = false, derive_default = true)]
    enable: Located<bool>,
}

fn main() {}
