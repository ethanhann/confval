//! The parsed argument type and the usage text.
//!
//! Argument parsing is handwritten so the binary takes no dependency the
//! library does not already have. A mandatory `clap` would put a command line
//! parser in the dependency tree of every library consumer.

use crate::install::{CliError, Scope};

/// The short usage synopsis, printed to stderr for a usage error and for the
/// bare command.
pub(crate) const USAGE: &str = "\
Usage:
  confval init [--agent <name>] [--scope <project|user>] [--force] [--launch]
  confval init --list
  confval --help
  confval --version";

/// The full help screen, printed to stdout for `--help` and `init --help`.
pub(crate) const HELP: &str = "\
confval installs the confval agent skills into a project.

It writes two skills, confval-init to scaffold a pipeline and confval-add-block
to keep the layers in sync, and reports what it wrote. It parses no
configuration and validates nothing.

Usage:
  confval init [options]
  confval init --list
  confval --help
  confval --version

Options:
  --agent <name>          the agent whose directory the skills install under,
                          only \"claude\", which selects .claude (default: claude)
  --scope <project|user>  install into the repository root (project) or the home
                          directory (user) (default: project)
  --force                 overwrite a file that differs from the shipped copy
  --launch                run the agent afterward in an interactive session
  --list                  list the skills and their descriptions, writing nothing
  -h, --help              print this help
  -V, --version           print the version

Example:
  confval init --launch

Exit codes:
  0  every file is present and current
  1  at least one file was skipped
  2  a usage error
  3  an IO error, an unresolved home directory, or a launch that failed
  4  the agent ran and exited non-zero";

/// The agent whose directory segment the skills install under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Agent {
    /// Claude Code, whose segment is `.claude`.
    Claude,
}

impl Agent {
    /// The directory segment for this agent.
    pub(crate) fn directory(&self) -> &'static str {
        match self {
            Agent::Claude => ".claude",
        }
    }

    /// The binary launched for this agent.
    pub(crate) fn binary(&self) -> &'static str {
        match self {
            Agent::Claude => "claude",
        }
    }

    fn parse(value: &str) -> Result<Agent, CliError> {
        match value {
            "claude" => Ok(Agent::Claude),
            other => Err(CliError::Usage(format!(
                "confval: unknown agent '{other}'. Supported agents: claude"
            ))),
        }
    }
}

/// The parsed `init` invocation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InitArgs {
    /// The agent whose segment the skills install under.
    pub(crate) agent: Agent,
    /// The scope that selects the base directory.
    pub(crate) scope: Scope,
    /// Overwrite a file that differs from the shipped copy.
    pub(crate) force: bool,
    /// Launch the agent after writing.
    pub(crate) launch: bool,
    /// List the skills and their descriptions instead of writing anything.
    pub(crate) list: bool,
}

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Print the usage text to stdout and exit 0.
    Help,
    /// Print the crate version to stdout and exit 0.
    Version,
    /// Install the skills, or list them when `list` is set.
    Init(InitArgs),
}

/// Parses the arguments after the program name.
pub(crate) fn parse(args: &[String]) -> Result<Command, CliError> {
    let mut iter = args.iter();
    let Some(first) = iter.next() else {
        return Err(CliError::Usage(USAGE.to_string()));
    };
    match first.as_str() {
        "--help" | "-h" => Ok(Command::Help),
        "--version" | "-V" => Ok(Command::Version),
        "init" => parse_init(iter),
        other => Err(CliError::Usage(format!(
            "confval: unknown subcommand '{other}'\n{USAGE}"
        ))),
    }
}

fn parse_init<'a>(mut iter: impl Iterator<Item = &'a String>) -> Result<Command, CliError> {
    let mut agent: Option<Agent> = None;
    let mut scope: Option<Scope> = None;
    let mut force = false;
    let mut launch = false;
    let mut list = false;

    while let Some(arg) = iter.next() {
        let (flag, inline) = split_flag(arg);
        match flag {
            "--help" | "-h" => return Ok(Command::Help),
            "--agent" => {
                if agent.is_some() {
                    return Err(duplicate("--agent"));
                }
                agent = Some(Agent::parse(value_for("--agent", inline, &mut iter)?)?);
            }
            "--scope" => {
                if scope.is_some() {
                    return Err(duplicate("--scope"));
                }
                scope = Some(parse_scope(value_for("--scope", inline, &mut iter)?)?);
            }
            "--force" => set_flag("--force", &mut force, inline)?,
            "--launch" => set_flag("--launch", &mut launch, inline)?,
            "--list" => set_flag("--list", &mut list, inline)?,
            "--" => {
                if let Some(extra) = iter.next() {
                    return Err(CliError::Usage(format!(
                        "confval: unexpected argument '{extra}'\n{USAGE}"
                    )));
                }
                break;
            }
            other => {
                return Err(CliError::Usage(format!(
                    "confval: unknown flag '{other}'\n{USAGE}"
                )));
            }
        }
    }

    if list && (force || launch) {
        return Err(CliError::Usage(format!(
            "confval: --list writes nothing and launches nothing, so it cannot be combined with --force or --launch\n{USAGE}"
        )));
    }

    Ok(Command::Init(InitArgs {
        agent: agent.unwrap_or(Agent::Claude),
        scope: scope.unwrap_or(Scope::Project),
        force,
        launch,
        list,
    }))
}

/// Splits `--flag=value` into the flag and its inline value. A token with no
/// `=`, or one that is not a long flag, has no inline value.
fn split_flag(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
        _ => (arg, None),
    }
}

/// The value for a flag that takes one, from the inline `=value` or the next
/// argument.
fn value_for<'a, I>(flag: &str, inline: Option<&'a str>, iter: &mut I) -> Result<&'a str, CliError>
where
    I: Iterator<Item = &'a String>,
{
    match inline {
        Some(value) => Ok(value),
        None => iter
            .next()
            .map(String::as_str)
            .ok_or_else(|| missing_value(flag)),
    }
}

/// Sets a boolean flag, rejecting a repeat and an inline value.
fn set_flag(flag: &str, slot: &mut bool, inline: Option<&str>) -> Result<(), CliError> {
    if inline.is_some() {
        return Err(CliError::Usage(format!(
            "confval: {flag} takes no value\n{USAGE}"
        )));
    }
    if *slot {
        return Err(duplicate(flag));
    }
    *slot = true;
    Ok(())
}

fn parse_scope(value: &str) -> Result<Scope, CliError> {
    match value {
        "project" => Ok(Scope::Project),
        "user" => Ok(Scope::User),
        other => Err(CliError::Usage(format!(
            "confval: unknown scope '{other}'. Supported scopes: project, user"
        ))),
    }
}

fn missing_value(flag: &str) -> CliError {
    CliError::Usage(format!("confval: {flag} needs a value\n{USAGE}"))
}

fn duplicate(flag: &str) -> CliError {
    CliError::Usage(format!("confval: {flag} given more than once\n{USAGE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_of(args: &[&str]) -> Result<Command, CliError> {
        let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        parse(&owned)
    }

    fn usage_message(result: Result<Command, CliError>) -> String {
        match result {
            Err(CliError::Usage(message)) => message,
            other => panic!("expected a usage error, got {other:?}"),
        }
    }

    fn init_of(args: &[&str]) -> InitArgs {
        match parse_of(args).unwrap() {
            Command::Init(init) => init,
            other => panic!("expected an init command, got {other:?}"),
        }
    }

    #[test]
    fn init_with_no_flags_uses_the_defaults() {
        // Arrange, Act
        let init = init_of(&["init"]);

        // Assert
        assert_eq!(
            init,
            InitArgs {
                agent: Agent::Claude,
                scope: Scope::Project,
                force: false,
                launch: false,
                list: false,
            }
        );
    }

    #[test]
    fn each_flag_parses() {
        // Arrange, Act
        let init = init_of(&[
            "init", "--agent", "claude", "--scope", "user", "--force", "--launch",
        ]);

        // Assert
        assert_eq!(init.agent, Agent::Claude);
        assert_eq!(init.scope, Scope::User);
        assert!(init.force);
        assert!(init.launch);
    }

    #[test]
    fn list_parses() {
        // Arrange, Act
        let init = init_of(&["init", "--list"]);

        // Assert
        assert!(init.list);
    }

    #[test]
    fn help_and_version_parse() {
        // Arrange, Act, Assert
        assert_eq!(parse_of(&["--help"]).unwrap(), Command::Help);
        assert_eq!(parse_of(&["--version"]).unwrap(), Command::Version);
    }

    #[test]
    fn bare_command_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&[]));

        // Assert
        assert_eq!(message, USAGE);
    }

    #[test]
    fn an_unknown_subcommand_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&["build"]));

        // Assert
        assert!(message.contains("unknown subcommand 'build'"));
    }

    #[test]
    fn an_unknown_flag_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--wat"]));

        // Assert
        assert!(message.contains("unknown flag '--wat'"));
    }

    #[test]
    fn an_unknown_agent_reports_the_pinned_message() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--agent", "codex"]));

        // Assert
        assert_eq!(
            message,
            "confval: unknown agent 'codex'. Supported agents: claude"
        );
    }

    #[test]
    fn an_unknown_scope_reports_the_pinned_message() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--scope", "global"]));

        // Assert
        assert_eq!(
            message,
            "confval: unknown scope 'global'. Supported scopes: project, user"
        );
    }

    #[test]
    fn a_flag_given_twice_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--force", "--force"]));

        // Assert
        assert!(message.contains("--force given more than once"));
    }

    #[test]
    fn a_flag_missing_its_value_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--agent"]));

        // Assert
        assert!(message.contains("--agent needs a value"));
    }

    #[test]
    fn list_with_force_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--list", "--force"]));

        // Assert
        assert!(message.contains("--list"));
    }

    #[test]
    fn list_with_launch_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--list", "--launch"]));

        // Assert
        assert!(message.contains("--list"));
    }

    #[test]
    fn an_inline_value_parses_like_a_spaced_one() {
        // Arrange, Act
        let init = init_of(&["init", "--agent=claude", "--scope=user"]);

        // Assert
        assert_eq!(init.agent, Agent::Claude);
        assert_eq!(init.scope, Scope::User);
    }

    #[test]
    fn init_help_flag_is_a_help_command() {
        // Arrange, Act, Assert
        assert_eq!(parse_of(&["init", "--help"]).unwrap(), Command::Help);
        assert_eq!(parse_of(&["init", "-h"]).unwrap(), Command::Help);
    }

    #[test]
    fn a_bare_double_dash_terminates_flags() {
        // Arrange, Act
        let init = init_of(&["init", "--force", "--"]);

        // Assert
        assert!(init.force);
    }

    #[test]
    fn a_boolean_flag_rejects_an_inline_value() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--force=yes"]));

        // Assert
        assert!(message.contains("--force takes no value"));
    }

    #[test]
    fn an_argument_after_the_terminator_is_a_usage_error() {
        // Arrange, Act
        let message = usage_message(parse_of(&["init", "--", "extra"]));

        // Assert
        assert!(message.contains("unexpected argument 'extra'"));
    }
}
