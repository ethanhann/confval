use confval::keyword_enum;

// Two variants mapping to the same keyword: `try_from("same")` could only ever
// yield the first, so the table must be rejected at compile time.
keyword_enum!(Dup, {
    A => "same",
    B => "same",
});

fn main() {}
