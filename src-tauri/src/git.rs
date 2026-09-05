//! Git exposure of an env file, derived by shelling out to `git`.
//!
//! This is deliberately dependency-free and read-only: it never stages,
//! ignores, or edits anything, and it never reads an env file's contents. A
//! missing or failing `git` is reported as [`EnvFileGitStatus::Unknown`] rather
//! than hidden, because "we could not tell" is a different answer from "this
//! file is safe".

use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvFileGitStatus {
    /// Git already has this file in its index; a commit can publish it.
    Tracked,
    UntrackedIgnored,
    /// Untracked and unignored: one `git add .` away from being committed.
    UntrackedNotIgnored,
    OutsideRepository,
    /// Git is absent, or a git invocation failed for a reason vne cannot read.
    Unknown,
}

impl EnvFileGitStatus {
    /// Short label for human text output.
    pub fn label(self) -> &'static str {
        match self {
            Self::Tracked => "git tracked",
            Self::UntrackedIgnored => "git ignored",
            Self::UntrackedNotIgnored => "git untracked",
            Self::OutsideRepository => "outside git",
            Self::Unknown => "git unknown",
        }
    }
}

/// Classifies one env file's exposure to git. Runs at most three fast plumbing
/// commands and never touches the file's contents.
pub fn env_file_git_status(path: &Path) -> EnvFileGitStatus {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return EnvFileGitStatus::Unknown;
    };

    match run_git(directory, &["rev-parse", "--is-inside-work-tree"]) {
        // A signal-killed git answered nothing, which is not the same answer as
        // "there is no repository here".
        GitInvocation::Missing | GitInvocation::Status(None) => return EnvFileGitStatus::Unknown,
        GitInvocation::Status(Some(0)) => {}
        GitInvocation::Status(Some(_)) => return EnvFileGitStatus::OutsideRepository,
    }

    match run_git(directory, &["ls-files", "--error-unmatch", "--", name]) {
        GitInvocation::Missing | GitInvocation::Status(None) => return EnvFileGitStatus::Unknown,
        GitInvocation::Status(Some(0)) => return EnvFileGitStatus::Tracked,
        GitInvocation::Status(Some(_)) => {}
    }

    match run_git(directory, &["check-ignore", "-q", "--", name]) {
        GitInvocation::Status(Some(0)) => EnvFileGitStatus::UntrackedIgnored,
        GitInvocation::Status(Some(1)) => EnvFileGitStatus::UntrackedNotIgnored,
        GitInvocation::Status(_) | GitInvocation::Missing => EnvFileGitStatus::Unknown,
    }
}

enum GitInvocation {
    /// `git` could not be started at all.
    Missing,
    /// `git` ran; `None` means it was killed by a signal.
    Status(Option<i32>),
}

/// Per-invocation deadline for git children. A git that exceeds it is
/// killed and reported as signal-death (`Status(None)` -> Unknown) — an
/// unobservable git is an honest Unknown, never a guess and never a hang
/// (VNE-SEC-014).
const GIT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

fn run_git(directory: &Path, arguments: &[&str]) -> GitInvocation {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        // Repo and global config cannot register execution for plumbing
        // (VNE-SEC-007): core.fsmonitor and friends are pinned off, system
        // config ignored. Plumbing never needs them.
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-c")
        .arg("core.splitIndex=false")
        .args(arguments)
        // Scrubbed environment: the child sees PATH (to find git) and
        // nothing else — caller environment is not an inheritance channel.
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return GitInvocation::Missing;
    };
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return GitInvocation::Status(status.code()),
            Ok(None) => {
                if started.elapsed() >= GIT_DEADLINE {
                    let _ = child.kill();
                    let _ = child.wait();
                    return GitInvocation::Status(None);
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return GitInvocation::Status(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn git(directory: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git should run in tests");
        assert!(status.success(), "git {arguments:?} failed");
    }

    fn scratch_repository() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        git(
            dir.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        git(dir.path(), &["config", "user.name", "vne test"]);
        dir
    }

    #[test]
    fn reports_each_git_exposure_state_from_a_scratch_repository() {
        let repository = scratch_repository();
        let root = repository.path();

        fs::write(root.join(".gitignore"), ".env.local\n").unwrap();
        fs::write(root.join(".env"), "PORT=1420\n").unwrap();
        fs::write(root.join(".env.local"), "PORT=1421\n").unwrap();
        fs::write(root.join(".env.production"), "PORT=1422\n").unwrap();
        git(root, &["add", ".gitignore", ".env"]);
        git(root, &["commit", "--quiet", "-m", "add env"]);

        assert_eq!(
            env_file_git_status(&root.join(".env")),
            EnvFileGitStatus::Tracked
        );
        assert_eq!(
            env_file_git_status(&root.join(".env.local")),
            EnvFileGitStatus::UntrackedIgnored
        );
        assert_eq!(
            env_file_git_status(&root.join(".env.production")),
            EnvFileGitStatus::UntrackedNotIgnored
        );
    }

    #[test]
    fn reports_a_nested_directory_against_the_same_repository() {
        let repository = scratch_repository();
        let nested = repository.path().join("services").join("api");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(".env"), "PORT=1420\n").unwrap();

        assert_eq!(
            env_file_git_status(&nested.join(".env")),
            EnvFileGitStatus::UntrackedNotIgnored
        );

        git(repository.path(), &["add", "services/api/.env"]);
        assert_eq!(
            env_file_git_status(&nested.join(".env")),
            EnvFileGitStatus::Tracked
        );
    }

    #[test]
    fn reports_a_file_outside_any_repository() {
        let dir = tempdir().unwrap();
        let env_file = dir.path().join(".env");
        fs::write(&env_file, "PORT=1420\n").unwrap();

        // A temporary directory can sit inside a repository on some machines,
        // so only assert the two answers that are honest here.
        assert!(matches!(
            env_file_git_status(&env_file),
            EnvFileGitStatus::OutsideRepository | EnvFileGitStatus::UntrackedNotIgnored
        ));
    }
}
