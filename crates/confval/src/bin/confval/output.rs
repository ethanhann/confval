//! Line output that treats a reader which has closed the pipe as a clean stop
//! rather than a panic.
//!
//! `println!` panics when it writes to a pipe whose reader has already closed,
//! as in `confval init | head`. These helpers write through `writeln!` and exit
//! cleanly on a broken pipe, so the binary stays a good pipeline citizen and
//! never panics on output.

use std::io::Write;

/// Writes a line to stdout, exiting cleanly if the reader has gone away.
pub(crate) fn line(args: std::fmt::Arguments) {
    if writeln!(std::io::stdout(), "{args}").is_err() {
        std::process::exit(0);
    }
}

/// Writes a line to stderr, ignoring a reader that has gone away.
pub(crate) fn eline(args: std::fmt::Arguments) {
    let _ = writeln!(std::io::stderr(), "{args}");
}
