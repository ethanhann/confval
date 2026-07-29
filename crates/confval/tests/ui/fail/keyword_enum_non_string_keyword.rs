use confval::keyword_enum;

// The keyword column must be string literals. A non-string literal passes the
// matcher, so this pins the type errors the expansion produces.
keyword_enum!(Bad, {
    One => 1,
});

fn main() {}
