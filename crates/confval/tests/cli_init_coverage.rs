//! Additional integration tests for the `confval` binary that cover the launch
//! outcomes and the broken pipe exit. Each test spawns the built binary with a
//! temporary working directory and `HOME`, and the launch tests place a fake
//! agent on `PATH` so no real agent is required.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("confval-cov-{tag}-{}-{n}", std::process::id()));
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

/// Writes a fake agent named `claude` into `bin_dir`. When executed it records
/// its physical working directory into `marker` and exits with `exit_code`.
#[cfg(unix)]
fn write_fake_agent(bin_dir: &Path, marker: &Path, exit_code: i32) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(bin_dir).unwrap();
    let claude = bin_dir.join("claude");
    let script = format!(
        "#!/bin/sh\npwd -P > \"{}\"\nexit {exit_code}\n",
        marker.display()
    );
    fs::write(&claude, script).unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn launch_under_user_scope_runs_the_agent_in_the_current_directory() {
    // Arrange
    let cwd = TempDir::new("launch-user-cwd");
    let home = TempDir::new("launch-user-home");
    let bin = TempDir::new("launch-user-bin");
    let marker = bin.path().join("workdir.txt");
    write_fake_agent(bin.path(), &marker, 0);

    // Act
    let output = Command::new(env!("CARGO_BIN_EXE_confval"))
        .args(["init", "--scope", "user", "--launch"])
        .current_dir(cwd.path())
        .env("HOME", home.path())
        .env("PATH", bin.path())
        .output()
        .unwrap();

    // Assert
    assert_eq!(output.status.code(), Some(0));
    let recorded = fs::canonicalize(fs::read_to_string(&marker).unwrap().trim()).unwrap();
    assert_eq!(recorded, fs::canonicalize(cwd.path()).unwrap());
}

#[cfg(unix)]
#[test]
fn launch_with_an_agent_that_exits_nonzero_exits_four() {
    // Arrange
    let dir = TempDir::new("launch-nonzero");
    let bin = TempDir::new("launch-nonzero-bin");
    let marker = bin.path().join("workdir.txt");
    write_fake_agent(bin.path(), &marker, 7);

    // Act
    let output = Command::new(env!("CARGO_BIN_EXE_confval"))
        .args(["init", "--launch"])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("PATH", bin.path())
        .output()
        .unwrap();

    // Assert
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("the agent exited with status 7"));
}

#[cfg(unix)]
#[test]
fn launch_with_a_non_executable_agent_exits_three() {
    // Arrange
    let dir = TempDir::new("launch-noexec");
    let bin = TempDir::new("launch-noexec-bin");
    fs::create_dir_all(bin.path()).unwrap();
    fs::write(bin.path().join("claude"), "not runnable\n").unwrap();

    // Act
    let output = Command::new(env!("CARGO_BIN_EXE_confval"))
        .args(["init", "--launch"])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("PATH", bin.path())
        .output()
        .unwrap();

    // Assert
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("could not launch the agent"));
}

#[cfg(unix)]
#[test]
fn a_reader_that_closes_the_pipe_makes_the_binary_exit_zero() {
    // Arrange
    let dir = TempDir::new("broken-pipe");
    let mut child = Command::new(env!("CARGO_BIN_EXE_confval"))
        .args(["init"])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    // Act
    drop(child.stdout.take());

    // Assert
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
}
