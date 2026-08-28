use confval::range_constraint;

// A `min:` above `max:` rejects every value at runtime. The macro asserts the
// order at compile time, and this pins the diagnostic an author sees.
range_constraint!(PORT, i64, min: 65535, max: 1);

fn main() {}
