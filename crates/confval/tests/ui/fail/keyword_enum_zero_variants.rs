use confval::keyword_enum;

// An empty table has no variants to generate. The macro matcher rejects it,
// and this pins the diagnostic an author sees.
keyword_enum!(Empty, {});

fn main() {}
