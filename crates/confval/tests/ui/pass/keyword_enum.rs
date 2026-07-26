//! `keyword_enum!` pass test.
//!
//! This pins the macro's public surface from outside its defining module.
//! A `keyword_set()` call resolves `$crate::KeywordSet` with no `KeywordSet`
//! import in scope, the `pub`, `pub(crate)`, and private visibilities all
//! compile, and a generated enum satisfies the `for<'a> TryFrom<&'a str>` bound
//! that `narrow::keyword` requires.

use confval::diagnostic::Report;
use confval::keyword_enum;
use confval::pipeline::narrow;
use confval::source::Located;

// `KeywordSet` is deliberately not imported. `keyword_set()` has to resolve it
// through `$crate::` inside the macro expansion.

keyword_enum!(pub LimitMode, {
    Enforce => "enforce",
    Log     => "log",
    Off     => "off",
});

keyword_enum!(pub(crate) Level, {
    Warn  => "warn",
    Error => "error",
});

keyword_enum!(Private, {
    On  => "on",
    Off => "off",
});

fn main() {
    let mut report = Report::new();

    // `pub` enum, full surface, no `KeywordSet` in scope.
    assert_eq!(LimitMode::KEYWORDS, ["enforce", "log", "off"]);
    assert_eq!(LimitMode::Log.as_str(), "log");
    assert_eq!(LimitMode::Off.to_string(), "off");
    assert_eq!(LimitMode::try_from("enforce"), Ok(LimitMode::Enforce));
    assert!(LimitMode::try_from("nope").is_err());
    LimitMode::keyword_set().check_located(
        &Located::detached("log".to_string()),
        "mode",
        &mut report,
    );
    assert!(!report.has_issues());

    // `pub(crate)` enum.
    assert_eq!(Level::KEYWORDS, ["warn", "error"]);
    assert_eq!(Level::Warn.as_str(), "warn");
    assert_eq!(Level::Error.to_string(), "error");
    assert_eq!(Level::try_from("warn"), Ok(Level::Warn));
    assert_eq!(Level::keyword_set().allowed.len(), 2);

    // Private enum.
    assert_eq!(Private::KEYWORDS, ["on", "off"]);
    assert_eq!(Private::On.as_str(), "on");
    assert_eq!(Private::Off.to_string(), "off");
    assert_eq!(Private::try_from("off"), Ok(Private::Off));
    assert_eq!(Private::keyword_set().allowed.len(), 2);

    // `narrow::keyword::<T>` composes with a generated enum through its
    // `for<'a> TryFrom<&'a str>` bound.
    let lowered =
        narrow::keyword::<LimitMode>(&Located::detached("enforce".to_string()), &mut report);
    assert_eq!(lowered, Some(LimitMode::Enforce));
}
