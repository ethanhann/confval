use confval::length_constraint;

// `help: None` was a forwarding spelling inside the macro and never a public
// form. This pins the diagnostic an author sees if they write it.
length_constraint!(NAME_LEN, min: 1, max: 2, help: None);

fn main() {}
