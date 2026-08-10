//! Integration tests for the `confval` binary. Each test spawns the built
//! binary with a temporary working directory and `HOME` pointed into a
//! temporary directory, so no test touches the real home directory or the
//! repository it runs in.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// The files the binary ships, at their path relative to the skills directory.
const FILES: &[&str] = &[
    "confval-init/SKILL.md",
    "confval-init/references/pipeline.md",
    "confval-init/references/frontends.md",
    "confval-init/references/patterns.md",
    "confval-add-block/SKILL.md",
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("confval-cli-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs the binary in `cwd` with `HOME` set to `home` and the given arguments.
fn run(cwd: &Path, home: &Path, args: &[&str]) -> Run {
    run_with_path(cwd, home, args, None)
}

/// Runs the binary with an optional `PATH` override, used by the launch tests.
fn run_with_path(cwd: &Path, home: &Path, args: &[&str], path: Option<&str>) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_confval"));
    command.args(args).current_dir(cwd).env("HOME", home);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let output = command.output().unwrap();
    Run {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// The bytes the binary is expected to write for a file, computed independently
/// of the binary by rendering the source template.
fn expected(relative_path: &str) -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills")
        .join(relative_path);
    fs::read_to_string(source)
        .unwrap()
        .replace("{{confval_version}}", env!("CARGO_PKG_VERSION"))
}

fn skills_dir(base: &Path) -> PathBuf {
    base.join(".claude").join("skills")
}

#[test]
fn a_clean_directory_gets_every_file_with_rendered_bytes() {
    // Arrange
    let dir = TempDir::new("clean");

    // Act
    let result = run(dir.path(), dir.path(), &["init"]);

    // Assert
    assert_eq!(result.code, 0);
    for relative in FILES {
        let installed = skills_dir(dir.path()).join(relative);
        assert_eq!(fs::read_to_string(&installed).unwrap(), expected(relative));
        assert!(result.stdout.contains(relative));
    }
}

#[test]
fn a_second_run_reports_unchanged_and_leaves_the_bytes_alone() {
    // Arrange
    let dir = TempDir::new("second");
    run(dir.path(), dir.path(), &["init"]);

    // Act
    let result = run(dir.path(), dir.path(), &["init"]);

    // Assert
    assert_eq!(result.code, 0);
    assert!(result.stdout.contains("unchanged"));
    let installed = skills_dir(dir.path()).join("confval-init/SKILL.md");
    assert_eq!(
        fs::read_to_string(&installed).unwrap(),
        expected("confval-init/SKILL.md")
    );
}

#[test]
fn an_edited_file_is_skipped_at_exit_one_with_the_edit_preserved() {
    // Arrange
    let dir = TempDir::new("edited");
    run(dir.path(), dir.path(), &["init"]);
    let edited = skills_dir(dir.path()).join("confval-init/SKILL.md");
    fs::write(&edited, "edited\n").unwrap();

    // Act
    let result = run(dir.path(), dir.path(), &["init"]);

    // Assert
    assert_eq!(result.code, 1);
    assert!(
        result
            .stdout
            .contains("skipped, differs from the copy this binary ships")
    );
    assert!(result.stderr.contains("Pass --force to overwrite."));
    assert_eq!(fs::read_to_string(&edited).unwrap(), "edited\n");
}

#[test]
fn force_over_an_edited_file_restores_the_shipped_bytes_at_exit_zero() {
    // Arrange
    let dir = TempDir::new("force");
    run(dir.path(), dir.path(), &["init"]);
    let edited = skills_dir(dir.path()).join("confval-init/SKILL.md");
    fs::write(&edited, "edited\n").unwrap();

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--force"]);

    // Assert
    assert_eq!(result.code, 0);
    assert!(result.stdout.contains("updated"));
    assert_eq!(
        fs::read_to_string(&edited).unwrap(),
        expected("confval-init/SKILL.md")
    );
}

#[test]
fn force_with_nothing_to_overwrite_reports_ordinary_outcomes_at_exit_zero() {
    // Arrange
    let dir = TempDir::new("force-clean");
    run(dir.path(), dir.path(), &["init"]);

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--force"]);

    // Assert
    assert_eq!(result.code, 0);
    assert!(result.stdout.contains("unchanged"));
    assert!(!result.stdout.contains("updated"));
}

#[test]
fn an_unrelated_file_in_a_skill_directory_survives() {
    // Arrange
    let dir = TempDir::new("unrelated");
    run(dir.path(), dir.path(), &["init"]);
    let unrelated = skills_dir(dir.path()).join("confval-init/notes.md");
    fs::write(&unrelated, "my notes\n").unwrap();

    // Act
    let result = run(dir.path(), dir.path(), &["init"]);

    // Assert
    assert_eq!(result.code, 0);
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "my notes\n");
    assert!(!result.stdout.contains("notes.md"));
}

#[test]
fn a_working_directory_below_git_installs_at_the_repository_root() {
    // Arrange
    let dir = TempDir::new("git-root");
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    let sub = dir.path().join("crates/api");
    fs::create_dir_all(&sub).unwrap();

    // Act
    let result = run(&sub, dir.path(), &["init"]);

    // Assert
    assert_eq!(result.code, 0);
    assert!(
        skills_dir(dir.path())
            .join("confval-init/SKILL.md")
            .exists()
    );
    assert!(!sub.join(".claude").exists());
    assert!(result.stdout.contains(".claude/skills"));
}

#[test]
fn a_working_directory_with_no_git_ancestor_installs_in_place() {
    // Arrange
    let dir = TempDir::new("no-git");

    // Act
    let result = run(dir.path(), dir.path(), &["init"]);

    // Assert
    assert_eq!(result.code, 0);
    assert!(
        skills_dir(dir.path())
            .join("confval-init/SKILL.md")
            .exists()
    );
}

#[test]
fn scope_user_installs_under_home() {
    // Arrange
    let cwd = TempDir::new("user-cwd");
    let home = TempDir::new("user-home");

    // Act
    let result = run(cwd.path(), home.path(), &["init", "--scope", "user"]);

    // Assert
    assert_eq!(result.code, 0);
    assert!(
        skills_dir(home.path())
            .join("confval-init/SKILL.md")
            .exists()
    );
    assert!(
        !skills_dir(cwd.path())
            .join("confval-init/SKILL.md")
            .exists()
    );
}

#[test]
fn list_writes_no_files_and_prints_both_names_with_descriptions() {
    // Arrange
    let dir = TempDir::new("list");

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--list"]);

    // Assert
    assert_eq!(result.code, 0);
    assert!(!dir.path().join(".claude").exists());
    assert!(result.stdout.contains("confval-init"));
    assert!(result.stdout.contains("confval-add-block"));
    assert!(result.stdout.contains("Scaffold a confval"));
    assert!(result.stdout.contains("Keep a confval pipeline in sync"));
}

#[test]
fn help_and_version_exit_zero() {
    // Arrange
    let dir = TempDir::new("help");

    // Act
    let help = run(dir.path(), dir.path(), &["--help"]);
    let version = run(dir.path(), dir.path(), &["--version"]);

    // Assert
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("Usage:"));
    assert_eq!(version.code, 0);
    assert_eq!(
        version.stdout.trim(),
        format!("confval {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn a_bare_command_exits_two() {
    // Arrange
    let dir = TempDir::new("bare");

    // Act
    let result = run(dir.path(), dir.path(), &[]);

    // Assert
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("Usage:"));
}

#[test]
fn an_unknown_subcommand_exits_two() {
    // Arrange
    let dir = TempDir::new("unknown-sub");

    // Act
    let result = run(dir.path(), dir.path(), &["build"]);

    // Assert
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("unknown subcommand 'build'"));
}

#[test]
fn an_unknown_flag_exits_two() {
    // Arrange
    let dir = TempDir::new("unknown-flag");

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--wat"]);

    // Assert
    assert_eq!(result.code, 2);
    assert!(result.stderr.contains("unknown flag '--wat'"));
}

#[test]
fn an_unknown_agent_exits_two_with_the_pinned_message() {
    // Arrange
    let dir = TempDir::new("unknown-agent");

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--agent", "codex"]);

    // Assert
    assert_eq!(result.code, 2);
    assert!(
        result
            .stderr
            .contains("confval: unknown agent 'codex'. Supported agents: claude")
    );
}

#[test]
fn an_unknown_scope_exits_two_with_the_pinned_message() {
    // Arrange
    let dir = TempDir::new("unknown-scope");

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--scope", "global"]);

    // Assert
    assert_eq!(result.code, 2);
    assert!(
        result
            .stderr
            .contains("confval: unknown scope 'global'. Supported scopes: project, user")
    );
}

#[test]
fn list_with_force_exits_two() {
    // Arrange
    let dir = TempDir::new("list-force");

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--list", "--force"]);

    // Assert
    assert_eq!(result.code, 2);
    assert!(!dir.path().join(".claude").exists());
}

#[test]
fn list_with_launch_exits_two() {
    // Arrange
    let dir = TempDir::new("list-launch");

    // Act
    let result = run(dir.path(), dir.path(), &["init", "--list", "--launch"]);

    // Assert
    assert_eq!(result.code, 2);
    assert!(!dir.path().join(".claude").exists());
}

#[test]
fn launch_after_a_skip_does_not_spawn_and_exits_one() {
    // Arrange
    let dir = TempDir::new("launch-skip");
    run(dir.path(), dir.path(), &["init"]);
    let edited = skills_dir(dir.path()).join("confval-init/SKILL.md");
    fs::write(&edited, "edited\n").unwrap();

    // Act
    let result = run_with_path(dir.path(), dir.path(), &["init", "--launch"], Some(""));

    // Assert
    assert_eq!(result.code, 1);
    assert!(result.stderr.contains("Pass --force to overwrite."));
}

#[test]
fn an_install_whose_parent_is_a_regular_file_exits_three() {
    // Arrange
    let dir = TempDir::new("parent-file");
    fs::write(dir.path().join(".claude"), "not a directory\n").unwrap();

    // Act
    let result = run(dir.path(), dir.path(), &["init"]);

    // Assert
    assert_eq!(result.code, 3);
    assert!(result.stderr.starts_with("confval:"));
}

#[test]
fn launch_with_an_unfindable_agent_exits_three_naming_path() {
    // Arrange
    let dir = TempDir::new("launch-nopath");

    // Act
    let result = run_with_path(dir.path(), dir.path(), &["init", "--launch"], Some(""));

    // Assert
    assert_eq!(result.code, 3);
    assert!(result.stderr.contains("PATH"));
}
