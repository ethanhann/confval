//! The confval binary, which installs the confval agent skills into a project.
//!
//! `confval init` writes the `confval-init` and `confval-add-block` skills into
//! the project's `.claude/skills/` directory and reports what it wrote. The
//! binary parses no configuration and validates nothing.

mod args;
mod commands;
mod install;
mod skills;

use args::Command;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&argv));
}

/// Parses the arguments, dispatches the command, and maps the result to an exit
/// code.
fn run(argv: &[String]) -> i32 {
    match args::parse(argv) {
        Ok(Command::Help) => {
            println!("{}", args::USAGE);
            0
        }
        Ok(Command::Version) => {
            println!("confval {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Ok(Command::Init(init)) => commands::init::run(&init).unwrap_or_else(|error| {
            eprintln!("{error}");
            error.exit_code()
        }),
        Err(error) => {
            eprintln!("{error}");
            error.exit_code()
        }
    }
}
