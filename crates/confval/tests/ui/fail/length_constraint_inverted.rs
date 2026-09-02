use confval::length_constraint;

// A `min:` above `max:` admits no count at runtime. The macro asserts the
// order at compile time. This test pins the diagnostic an author sees.
length_constraint!(NAME_LEN, min: 10, max: 1);

fn main() {}
