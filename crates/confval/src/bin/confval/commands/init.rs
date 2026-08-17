//! Orchestrates install, report, and the optional launch.

use crate::args::InitArgs;
use crate::install::{self, CliError, Scope};
use crate::output;
use crate::skills::{self, SKILLS};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Runs `confval init`. Returns the process exit code, which is 0 for a clean
/// run and 1 when a file was skipped.
pub(crate) fn run(init: &InitArgs) -> Result<i32, CliError> {
    if init.list {
        return list();
    }

    let cwd = current_dir()?;
    let base = install::base(init.scope, &cwd)?;
    let skills_dir = base.join(init.agent.directory()).join("skills");
    let width = column_width();

    output::line(format_args!(
        "installed confval skills into {}",
        skills_dir.display()
    ));

    let mut any_skip = false;
    for skill in SKILLS {
        for file in skill.files() {
            let dest = skills_dir.join(file.relative_path);
            let rendered = skills::render(file);
            let outcome = install::plan(&dest, &rendered, init.force)?;
            install::apply(&dest, &rendered, &outcome)?;
            output::line(format_args!(
                "  {:<width$}{}",
                file.relative_path,
                outcome.label()
            ));
            any_skip |= outcome.is_skip();
        }
    }

    if any_skip {
        output::eline(format_args!(""));
        output::eline(format_args!("Pass --force to overwrite."));
        return Ok(1);
    }

    if init.launch {
        return launch(init, &base, &cwd);
    }

    output::line(format_args!(""));
    output::line(format_args!(
        "Run claude in this project and invoke /confval-init, or rerun with --launch."
    ));
    Ok(0)
}

/// Prints each skill name and its description, and writes nothing.
fn list() -> Result<i32, CliError> {
    for skill in SKILLS {
        let description = skills::description(skill.skill_md.template).unwrap_or("");
        output::line(format_args!("{}", skill.name));
        output::line(format_args!("  {description}"));
    }
    Ok(0)
}

/// Launches the agent, inheriting stdio, and waits for it.
///
/// The working directory is the project base under project scope and the
/// current directory under user scope, because a user-scope install says
/// nothing about which project to set up.
fn launch(init: &InitArgs, base: &Path, cwd: &Path) -> Result<i32, CliError> {
    let workdir = match init.scope {
        Scope::Project => base,
        Scope::User => cwd,
    };
    let status = std::process::Command::new(init.agent.binary())
        .arg("Set up confval in this project. Use the confval-init skill.")
        .current_dir(workdir)
        .status();
    match status {
        Ok(status) if status.success() => Ok(0),
        Ok(status) => Err(CliError::AgentStatus(status.code().unwrap_or(1))),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Err(CliError::AgentNotFound(init.agent.binary().to_string()))
        }
        Err(error) => Err(CliError::AgentSpawn(error)),
    }
}

/// The width the outcome column aligns to, the longest relative path plus two
/// spaces.
fn column_width() -> usize {
    SKILLS
        .iter()
        .flat_map(|skill| skill.files())
        .map(|file| file.relative_path.len())
        .max()
        .unwrap_or(0)
        + 2
}

fn current_dir() -> Result<PathBuf, CliError> {
    std::env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })
}
