use confval::prelude::*;

length_constraint!(NAME_LEN, min: 1, max: 63);

keyword_enum!(LimitMode, {
    Enforce => "enforce",
    Log     => "log",
});

#[derive(confval::Spec)]
struct Cfg {
    #[confval(keywords = LimitMode, length = NAME_LEN)]
    mode: Located<String>,
}

fn main() {}
