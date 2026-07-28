use confval::source::Located;

#[derive(confval::Spec)]
struct InnerSpec {
    name: Located<String>,
}

// A bare `#[confval(default)]` on an optional nested field is the populate
// marker: `to_fields` fills an absent block from `InnerSpec::default()`. A
// `default = expr` on a whole block has no meaning, so the derive rejects it.
#[derive(confval::Spec)]
struct ServerSpec {
    #[confval(nested, default = 1)]
    inner: Option<Located<InnerSpec>>,
}

fn main() {}
