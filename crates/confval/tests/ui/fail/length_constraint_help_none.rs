use confval::length_constraint;

// `help: None` forwarded to another arm inside the macro. No page documented
// it. This pins the diagnostic an author sees if they write it.
length_constraint!(NAME_LEN, min: 1, max: 2, help: None);

fn main() {}
