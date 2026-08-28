use confval::length_constraint;

// `help: None` was a forwarding form inside the macro and was never documented.
// This pins the diagnostic an author sees if they write it.
length_constraint!(NAME_LEN, min: 1, max: 2, help: None);

fn main() {}
