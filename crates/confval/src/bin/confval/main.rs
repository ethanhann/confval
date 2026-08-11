//! The confval binary, which installs the confval agent skills into a project.
//!
//! `confval init` writes the `confval-init` and `confval-add-block` skills into
//! the project's `.claude/skills/` directory and reports what it wrote. The
//! binary parses no configuration and validates nothing.

mod args;
mod commands;
mod install;
mod output;
mod skills;

use args::Command;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}

fn run(argv: &[String]) -> i32 {
    match args::parse(argv) {
        Ok(Command::Help) => {
            output::line(format_args!("{}", args::HELP));
            0
        }
        Ok(Command::Version) => {
            output::line(format_args!("confval {}", env!("CARGO_PKG_VERSION")));
            0
        }
        Ok(Command::Init(init)) => commands::init::run(&init).unwrap_or_else(|error| {
            output::eline(format_args!("{error}"));
            error.exit_code()
        }),
        Err(error) => {
            output::eline(format_args!("{error}"));
            error.exit_code()
        }
    }
}
