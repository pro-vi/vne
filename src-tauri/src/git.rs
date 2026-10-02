//! Git exposure of an env file, derived by shelling out to `git`.
//!
//! This is deliberately dependency-free and read-only: it never stages,
//! ignores, or edits anything, and it never reads an env file's contents. A
//! missing, failing, or too slow `git` is reported as [`EnvFileGitStatus::Unknown`] rather
//! than hidden, because "we could not tell" is a different answer from "this
//! file is safe".

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvFileGitStatus {
    /// Git already has this file in its index; a commit can publish it.
    Tracked,
    UntrackedIgnored,
    /// Untracked and unignored: one `git add .` away from being committed.
    UntrackedNotIgnored,
    OutsideRepository,
    /// vne could not tell: git is absent, failed, refused to open the
    /// repository, or was cut off by a deadline; git's listings left the file
    /// out (a case-only rename, a file inside an uninitialized submodule); the
    /// file name is not UTF-8; or the loader's `GitStatusScope` did not ask.
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

/// Classifies env files' exposure to git, in the order given: per parent
/// directory, one `ls-files` listing for its ASCII names and one per name that
/// is not ASCII, a second listing to confirm files the first leaves out, and a
/// `rev-parse` only when a listing is refused. Once `deadline` passes, no
/// further git command starts and the files still waiting keep `Unknown`.
pub fn env_file_git_statuses(
    paths: &[&Path],
    deadline: Option<Instant>,
) -> Vec<EnvFileGitStatus> {
    let mut statuses = vec![EnvFileGitStatus::Unknown; paths.len()];
    let mut directories: Vec<(&Path, Vec<(usize, &str)>)> = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let Some((directory, name)) = directory_and_name(path) else {
            continue;
        };
        match directories.iter_mut().find(|(seen, _)| *seen == directory) {
            Some((_, members)) => members.push((index, name)),
            None => directories.push((directory, vec![(index, name)])),
        }
    }

    for (directory, members) in directories {
        let names = members.iter().map(|(_, name)| *name).collect::<Vec<_>>();
        let found = directory_git_statuses(directory, &names, deadline);
        for ((index, _), status) in members.iter().zip(found) {
            statuses[*index] = status;
        }
    }
    statuses
}

/// Whether git tracks this file, from one plumbing command. Anything short of
/// a clear yes, including no repository and a failing git, is false.
pub fn env_file_is_tracked(path: &Path) -> bool {
    let Some((directory, name)) = directory_and_name(path) else {
        return false;
    };
    let arguments = ["--literal-pathspecs", "ls-files", "--error-unmatch", "--", name];
    matches!(run_git(directory, &arguments, None), Some((0, _)))
}

fn directory_and_name(path: &Path) -> Option<(&Path, &str)> {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    Some((directory, path.file_name()?.to_str()?))
}

fn directory_git_statuses(
    directory: &Path,
    names: &[&str],
    deadline: Option<Instant>,
) -> Vec<EnvFileGitStatus> {
    // git prints an ASCII name back byte for byte, so one listing answers all
    // of them. git 2.50.1 on macOS prints a non-ASCII name precomposed, which
    // can differ from the bytes on disk, so each gets a listing of its own and
    // is answered by whether anything was listed.
    let (ascii, other): (Vec<usize>, Vec<usize>) =
        (0..names.len()).partition(|index| names[*index].is_ascii());
    let mut queries = vec![ascii];
    queries.retain(|query| !query.is_empty());
    queries.extend(other.into_iter().map(|index| vec![index]));

    let mut statuses = vec![EnvFileGitStatus::Unknown; names.len()];
    for query in queries {
        let by_bytes = names[query[0]].is_ascii();
        let find = |listed: &BTreeMap<String, u8>, name: &str| {
            if by_bytes {
                listed.get(name).copied()
            } else {
                listed.values().next().copied()
            }
        };
        let asked = query.iter().map(|index| names[*index]).collect::<Vec<_>>();
        let listed = match list_files(directory, &asked, VISIBLE, deadline) {
            Some((0, stdout)) => parse_listing(&stdout),
            // `ls-files` refuses outside a repository, in one git refuses to
            // open (dubious ownership), and in one whose index it cannot read;
            // `rev-parse` exits 0 only in the last case (git 2.50.1). When both
            // refuse, a `.git` entry at or above the directory means git would
            // not open a repository that is there, which is not an answer.
            Some(_) => {
                let probe = ["rev-parse", "--is-inside-work-tree"];
                if let Some((1.., _)) = run_git(directory, &probe, deadline) {
                    if !has_git_entry_above(directory) {
                        return vec![EnvFileGitStatus::OutsideRepository; names.len()];
                    }
                }
                return statuses;
            }
            // No answer from git; these files stay `Unknown`.
            None => continue,
        };

        let mut unlisted = Vec::new();
        for index in query {
            match find(&listed, names[index]) {
                Some(b'?') => statuses[index] = EnvFileGitStatus::UntrackedNotIgnored,
                Some(_) => statuses[index] = EnvFileGitStatus::Tracked,
                None => unlisted.push(index),
            }
        }
        // A file is also left out after a case-only rename or inside an
        // uninitialized submodule (git 2.50.1), so it counts as ignored only
        // when git lists it as ignored; otherwise it stays `Unknown`.
        if unlisted.is_empty() {
            continue;
        }
        let asked = unlisted.iter().map(|index| names[*index]).collect::<Vec<_>>();
        if let Some((0, stdout)) = list_files(directory, &asked, IGNORED, deadline) {
            let ignored = parse_listing(&stdout);
            for index in unlisted {
                if find(&ignored, names[index]).is_some() {
                    statuses[index] = EnvFileGitStatus::UntrackedIgnored;
                }
            }
        }
    }
    statuses
}

/// Tracked files, tagged by letter, and untracked files git does not ignore,
/// tagged `?`.
const VISIBLE: &[&str] = &["--cached", "--others", "--exclude-standard"];
/// Untracked files git ignores.
const IGNORED: &[&str] = &["--others", "--ignored", "--exclude-standard"];

/// Lists the named files with their `ls-files -t` tag; `selection` chooses
/// which files git lists.
fn list_files(
    directory: &Path,
    names: &[&str],
    selection: &[&str],
    deadline: Option<Instant>,
) -> Option<(i32, Vec<u8>)> {
    let mut arguments = vec!["--literal-pathspecs", "ls-files", "-z", "-t"];
    arguments.extend(selection);
    arguments.push("--");
    arguments.extend(names);
    run_git(directory, &arguments, deadline)
}

fn has_git_entry_above(directory: &Path) -> bool {
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    directory
        .ancestors()
        .any(|ancestor| ancestor.join(".git").exists())
}

fn parse_listing(stdout: &[u8]) -> BTreeMap<String, u8> {
    stdout
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let (tag, rest) = entry.split_first()?;
            let name = rest.strip_prefix(b" ")?;
            Some((String::from_utf8_lossy(name).into_owned(), *tag))
        })
        .collect()
}

/// Per-invocation deadline for git children. A git that exceeds it is
/// killed and gives no answer (-> Unknown) — an
/// unobservable git is an honest Unknown, never a guess and never a hang
/// (VNE-SEC-014).
const GIT_DEADLINE: Duration = Duration::from_secs(5);

/// Cap git's stdout; a listing that reaches the limit may be truncated and
/// cannot provide a reliable status.
const GIT_OUTPUT_LIMIT: u64 = 1024 * 1024;

/// Runs one git command and returns its exit code and stdout, or `None` when
/// git gave no answer: it could not start, the deadline had passed, it was
/// killed or timed out, a signal ended it, or its output was cut off.
fn run_git(
    directory: &Path,
    arguments: &[&str],
    deadline: Option<Instant>,
) -> Option<(i32, Vec<u8>)> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return None;
    }
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
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;

    // Read on a thread that is never joined: a child of git that holds the
    // pipe open after git is killed must not hold vne past the deadline.
    let (sender, receiver) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let read = stdout.take(GIT_OUTPUT_LIMIT).read_to_end(&mut output);
            let _ = sender.send(read.map(|_| output));
        });
    }

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let remaining = GIT_DEADLINE.saturating_sub(started.elapsed());
                return match receiver.recv_timeout(remaining) {
                    Ok(Ok(stdout)) if (stdout.len() as u64) < GIT_OUTPUT_LIMIT => {
                        status.code().map(|code| (code, stdout))
                    }
                    _ => None,
                };
            }
            // A plumbing call takes about 12 ms (git 2.50.1, M5 Max); in a
            // 50-file scan, polling every 1 ms used the same CPU time as a
            // blocking wait.
            Ok(None) if started.elapsed() < GIT_DEADLINE => {
                std::thread::sleep(Duration::from_millis(1));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
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

    fn env_file_git_status(path: &Path) -> EnvFileGitStatus {
        env_file_git_statuses(&[path], None)[0]
    }

    fn scratch_repository() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        dir
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
    fn classifies_files_across_directories_in_batches() {
        let repository = scratch_repository();
        let root = repository.path();
        let nested = root.join("services");
        fs::create_dir(&nested).unwrap();
        fs::write(root.join(".gitignore"), ".env.local\nignored \"q\".env\n").unwrap();
        let names = [".env", ".env.local", ".env.production", "ignored \"q\".env", "tracked \u{e9}.env"];
        for name in names {
            fs::write(root.join(name), "K=v\n").unwrap();
        }
        fs::write(nested.join(".env"), "K=v\n").unwrap();
        git(root, &["add", ".gitignore", ".env", "tracked \u{e9}.env"]);

        let mut paths = names.iter().map(|name| root.join(name)).collect::<Vec<_>>();
        paths.push(nested.join(".env"));
        let paths = paths.iter().map(|path| path.as_path()).collect::<Vec<_>>();

        assert_eq!(
            env_file_git_statuses(&paths, None),
            [
                EnvFileGitStatus::Tracked,
                EnvFileGitStatus::UntrackedIgnored,
                EnvFileGitStatus::UntrackedNotIgnored,
                EnvFileGitStatus::UntrackedIgnored,
                EnvFileGitStatus::Tracked,
                EnvFileGitStatus::UntrackedNotIgnored,
            ]
        );
    }

    #[test]
    fn a_passed_deadline_leaves_every_file_unknown() {
        let repository = scratch_repository();
        let env_file = repository.path().join(".env");
        fs::write(&env_file, "K=v\n").unwrap();

        assert_eq!(
            env_file_git_statuses(&[env_file.as_path()], Some(Instant::now())),
            [EnvFileGitStatus::Unknown]
        );
    }

    #[test]
    fn reports_tracking_with_one_command() {
        let repository = scratch_repository();
        let root = repository.path();
        fs::write(root.join(".env"), "K=v\n").unwrap();
        fs::write(root.join(".env.local"), "K=v\n").unwrap();
        git(root, &["add", ".env"]);

        assert!(env_file_is_tracked(&root.join(".env")));
        assert!(!env_file_is_tracked(&root.join(".env.local")));
    }

    #[test]
    fn pathspec_magic_names_neither_fail_nor_skew_their_directory() {
        let repository = scratch_repository();
        let root = repository.path();
        fs::write(root.join(".gitignore"), "/prod.env\n").unwrap();
        let names = ["plain.env", ":(bad)x.env", ":!neg.env", ":prod.env"];
        for name in names {
            fs::write(root.join(name), "K=v\n").unwrap();
        }
        let paths = names.iter().map(|name| root.join(name)).collect::<Vec<_>>();
        let paths = paths.iter().map(|path| path.as_path()).collect::<Vec<_>>();

        assert_eq!(
            env_file_git_statuses(&paths, None),
            [EnvFileGitStatus::UntrackedNotIgnored; 4]
        );
    }

    /// git (2.50.1) precomposes Unicode on macOS, so a name stored decomposed
    /// on disk differs in bytes from the index path git prints.
    #[cfg(target_os = "macos")]
    #[test]
    fn decomposed_unicode_names_keep_their_git_status() {
        let repository = scratch_repository();
        let root = repository.path();
        fs::write(root.join(".gitignore"), "na\u{ef}ve.env\n").unwrap();
        let tracked = root.join("cafe\u{301}.env");
        let ignored = root.join("nai\u{308}ve.env");
        fs::write(&tracked, "K=v\n").unwrap();
        fs::write(&ignored, "K=v\n").unwrap();
        git(root, &["add", "--", "cafe\u{301}.env"]);

        assert_eq!(
            env_file_git_statuses(&[tracked.as_path(), ignored.as_path()], None),
            [EnvFileGitStatus::Tracked, EnvFileGitStatus::UntrackedIgnored]
        );
    }

    #[test]
    fn a_file_left_out_of_the_listing_is_not_called_ignored_without_proof() {
        let repository = scratch_repository();
        let root = repository.path();
        fs::write(root.join(".env.Local"), "K=v\n").unwrap();
        git(root, &["add", ".env.Local"]);
        // A case-only rename: git still tracks `.env.Local`, and on a
        // case-insensitive filesystem the listing names neither spelling
        // (git 2.50.1).
        fs::rename(root.join(".env.Local"), root.join(".env.local")).unwrap();

        assert_ne!(
            env_file_git_status(&root.join(".env.local")),
            EnvFileGitStatus::UntrackedIgnored
        );
    }

    #[test]
    fn a_repository_git_refuses_to_open_is_unknown_not_outside() {
        let repository = scratch_repository();
        let root = repository.path();
        fs::write(root.join(".env"), "K=v\n").unwrap();
        git(root, &["add", ".env"]);
        // vne runs git without HOME, so git (2.50.1) cannot expand `~` here and
        // refuses the repository, as it does one with dubious ownership.
        git(root, &["config", "core.excludesFile", "~/ignore"]);

        assert_eq!(
            env_file_git_status(&root.join(".env")),
            EnvFileGitStatus::Unknown
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
