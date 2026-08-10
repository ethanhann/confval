//! Base resolution, the plan and apply pair, and the error type.
//!
//! `plan` decides an outcome for one file without touching the file system
//! beyond a read, and `apply` carries it out. Splitting the two is what lets the
//! outcome table be unit tested without a spawned process.

use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Where the skills are installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The repository the working directory belongs to.
    Project,
    /// The user's home directory.
    User,
}

/// What happens to one file, decided by comparing the bytes on disk with the
/// bytes this binary would write.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// No file at the path. The file is written.
    Created,
    /// The file matches what this binary writes. Nothing is written.
    Unchanged,
    /// The file differs and `--force` was not given. Nothing is written.
    Skipped,
    /// The file differs and `--force` was given. The file is written.
    Updated,
}

impl Outcome {
    /// The word that follows the path in the report.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Outcome::Created => "created",
            Outcome::Unchanged => "unchanged",
            Outcome::Skipped => "skipped, differs from the copy this binary ships",
            Outcome::Updated => "updated",
        }
    }

    /// Whether this outcome is a skip, which makes the run exit 1 and suppresses
    /// a launch.
    pub(crate) fn is_skip(&self) -> bool {
        matches!(self, Outcome::Skipped)
    }
}

/// Everything that can go wrong, carrying its own exit code.
#[derive(Debug)]
pub(crate) enum CliError {
    /// A usage error, including an unknown flag, agent, or scope.
    Usage(String),
    /// The home directory could not be determined under user scope.
    NoHome,
    /// A file system read or write failed.
    Io {
        /// The path being read or written.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The agent binary was not found on `PATH`.
    AgentNotFound(String),
    /// The agent binary could not be launched for some other reason.
    AgentSpawn(std::io::Error),
    /// The agent ran and exited non-zero.
    AgentStatus(i32),
}

impl CliError {
    /// The process exit code for this error.
    pub(crate) fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage(_) => 2,
            CliError::NoHome
            | CliError::Io { .. }
            | CliError::AgentNotFound(_)
            | CliError::AgentSpawn(_) => 3,
            CliError::AgentStatus(_) => 4,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(message) => write!(f, "{message}"),
            CliError::NoHome => write!(f, "confval: could not determine the home directory"),
            CliError::Io { path, source } => {
                write!(f, "confval: {}: {source}", path.display())
            }
            CliError::AgentNotFound(bin) => {
                write!(
                    f,
                    "confval: '{bin}' not found. Is it installed and on PATH?"
                )
            }
            CliError::AgentSpawn(source) => {
                write!(f, "confval: could not launch the agent: {source}")
            }
            CliError::AgentStatus(code) => {
                write!(f, "confval: the agent exited with status {code}")
            }
        }
    }
}

/// The directory the skills install under, before the `.claude/skills` segments.
///
/// Under project scope this is the repository root. Under user scope it is the
/// home directory.
pub(crate) fn base(scope: Scope, cwd: &Path) -> Result<PathBuf, CliError> {
    base_with_home(scope, cwd, std::env::home_dir())
}

/// The base resolution with the home lookup passed in, so the user-scope and
/// `NoHome` arms are testable without touching the environment.
fn base_with_home(scope: Scope, cwd: &Path, home: Option<PathBuf>) -> Result<PathBuf, CliError> {
    match scope {
        Scope::Project => Ok(project_base(cwd)),
        Scope::User => home.ok_or(CliError::NoHome),
    }
}

/// The nearest ancestor of `cwd` that contains a `.git` entry, or `cwd` itself
/// when no ancestor has one.
///
/// A `.git` file, which a worktree or submodule carries, is treated the same as
/// a `.git` directory, because `Path::exists` follows both.
fn project_base(cwd: &Path) -> PathBuf {
    for dir in cwd.ancestors() {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

/// Decides an outcome without touching the file system beyond a read.
pub(crate) fn plan(path: &Path, rendered: &str, force: bool) -> Result<Outcome, CliError> {
    match fs::read(path) {
        Ok(existing) if existing == rendered.as_bytes() => Ok(Outcome::Unchanged),
        Ok(_) if force => Ok(Outcome::Updated),
        Ok(_) => Ok(Outcome::Skipped),
        Err(source) if source.kind() == ErrorKind::NotFound => Ok(Outcome::Created),
        Err(source) => Err(CliError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Writes the file when the outcome is `Created` or `Updated`, and does nothing
/// otherwise.
pub(crate) fn apply(path: &Path, rendered: &str, outcome: &Outcome) -> Result<(), CliError> {
    match outcome {
        Outcome::Created | Outcome::Updated => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| CliError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(path, rendered).map_err(|source| CliError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
        Outcome::Unchanged | Outcome::Skipped => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("confval-{tag}-{}-{n}", std::process::id()));
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

    #[test]
    fn plan_reports_created_for_an_absent_path() {
        // Arrange
        let dir = TempDir::new("plan-absent");
        let path = dir.path().join("SKILL.md");

        // Act
        let outcome = plan(&path, "body", false).unwrap();

        // Assert
        assert_eq!(outcome, Outcome::Created);
    }

    #[test]
    fn plan_reports_unchanged_for_identical_content() {
        // Arrange
        let dir = TempDir::new("plan-same");
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "body").unwrap();

        // Act
        let outcome = plan(&path, "body", false).unwrap();

        // Assert
        assert_eq!(outcome, Outcome::Unchanged);
    }

    #[test]
    fn plan_reports_unchanged_even_with_force() {
        // Arrange
        let dir = TempDir::new("plan-same-force");
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "body").unwrap();

        // Act
        let outcome = plan(&path, "body", true).unwrap();

        // Assert
        assert_eq!(outcome, Outcome::Unchanged);
    }

    #[test]
    fn plan_reports_skipped_for_differing_content_without_force() {
        // Arrange
        let dir = TempDir::new("plan-diff");
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "edited").unwrap();

        // Act
        let outcome = plan(&path, "body", false).unwrap();

        // Assert
        assert_eq!(outcome, Outcome::Skipped);
    }

    #[test]
    fn plan_reports_updated_for_differing_content_with_force() {
        // Arrange
        let dir = TempDir::new("plan-diff-force");
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "edited").unwrap();

        // Act
        let outcome = plan(&path, "body", true).unwrap();

        // Assert
        assert_eq!(outcome, Outcome::Updated);
    }

    #[test]
    fn plan_reports_created_for_an_absent_path_even_with_force() {
        // Arrange
        let dir = TempDir::new("plan-absent-force");
        let path = dir.path().join("SKILL.md");

        // Act
        let outcome = plan(&path, "body", true).unwrap();

        // Assert
        assert_eq!(outcome, Outcome::Created);
    }

    #[test]
    fn apply_writes_a_created_file_and_its_parents() {
        // Arrange
        let dir = TempDir::new("apply-created");
        let path = dir.path().join("nested/SKILL.md");

        // Act
        apply(&path, "body", &Outcome::Created).unwrap();

        // Assert
        assert_eq!(fs::read_to_string(&path).unwrap(), "body");
    }

    #[test]
    fn apply_leaves_a_skipped_file_untouched() {
        // Arrange
        let dir = TempDir::new("apply-skip");
        let path = dir.path().join("SKILL.md");
        fs::write(&path, "edited").unwrap();

        // Act
        apply(&path, "body", &Outcome::Skipped).unwrap();

        // Assert
        assert_eq!(fs::read_to_string(&path).unwrap(), "edited");
    }

    #[test]
    fn base_project_walks_up_to_a_git_directory() {
        // Arrange
        let dir = TempDir::new("base-git-dir");
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        let sub = dir.path().join("crates/api");
        fs::create_dir_all(&sub).unwrap();

        // Act
        let resolved = base(Scope::Project, &sub).unwrap();

        // Assert
        assert_eq!(resolved, dir.path());
    }

    #[test]
    fn base_project_treats_a_git_file_like_a_git_directory() {
        // Arrange
        let dir = TempDir::new("base-git-file");
        fs::write(dir.path().join(".git"), "gitdir: /elsewhere\n").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir_all(&sub).unwrap();

        // Act
        let resolved = base(Scope::Project, &sub).unwrap();

        // Assert
        assert_eq!(resolved, dir.path());
    }

    #[test]
    fn base_project_falls_back_to_the_working_directory() {
        // Arrange
        let dir = TempDir::new("base-no-git");

        // Act
        let resolved = base(Scope::Project, dir.path()).unwrap();

        // Assert
        assert_eq!(resolved, dir.path());
    }

    #[test]
    fn base_user_returns_the_home_directory() {
        // Arrange
        let home = PathBuf::from("/home/e");
        let cwd = Path::new("/anywhere");

        // Act
        let resolved = base_with_home(Scope::User, cwd, Some(home.clone())).unwrap();

        // Assert
        assert_eq!(resolved, home);
    }

    #[test]
    fn base_user_without_a_home_is_no_home() {
        // Arrange
        let cwd = Path::new("/anywhere");

        // Act
        let resolved = base_with_home(Scope::User, cwd, None);

        // Assert
        assert!(matches!(resolved, Err(CliError::NoHome)));
    }

    #[test]
    fn each_error_arm_maps_to_its_exit_code() {
        // Arrange
        let io = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);

        // Assert
        assert_eq!(CliError::Usage(String::new()).exit_code(), 2);
        assert_eq!(CliError::NoHome.exit_code(), 3);
        assert_eq!(
            CliError::Io {
                path: PathBuf::from("x"),
                source: io(),
            }
            .exit_code(),
            3
        );
        assert_eq!(CliError::AgentNotFound("claude".into()).exit_code(), 3);
        assert_eq!(CliError::AgentSpawn(io()).exit_code(), 3);
        assert_eq!(CliError::AgentStatus(7).exit_code(), 4);
    }
}
