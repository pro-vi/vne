use std::path::Path;
use std::process::Command;

fn main() {
    record_git_commit();
    tauri_build::build()
}

/// Bakes the commit this binary was built from into `vne --version`, marked
/// `-dirty` when tracked files differ from it, so a bug report can name the
/// exact build. A source tree git cannot describe, such as a source archive or
/// a copy inside another repository, reports the crate version alone.
fn record_git_commit() {
    if git(&["ls-files", "--error-unmatch", "build.rs"]).is_none() {
        return;
    }
    let Some(commit) = git(&["rev-parse", "HEAD"]) else {
        return;
    };
    let Some(changes) = git(&["status", "--porcelain", "--untracked-files=no"]) else {
        return;
    };
    let suffix = if changes.is_empty() { "" } else { "-dirty" };
    println!("cargo:rustc-env=VNE_GIT_COMMIT={commit}{suffix}");

    // The marker is computed only when this script runs, and `cargo install
    // --path`, which `npm run install:local` uses, reuses src-tauri/target
    // (cargo 1.94.1). A commit or checkout moves HEAD or the branch ref; an
    // edit moves neither, so the binary's inputs are watched too: the Rust
    // sources, the lockfile, and the frontend build it embeds. The index is
    // not watched, because every `git add` or `git status` elsewhere would
    // then force a rebuild.
    let mut ref_args = vec!["rev-parse", "--git-path", "HEAD", "--git-path", "packed-refs"];
    let branch = git(&["symbolic-ref", "-q", "HEAD"]);
    if let Some(branch) = &branch {
        ref_args.extend(["--git-path", branch]);
    }
    let ref_paths = git(&ref_args).unwrap_or_default();
    let watched = ref_paths
        .lines()
        .map(str::to_string)
        .chain(["src", "Cargo.lock", "../dist"].map(String::from));
    for path in watched {
        // Cargo (as of 1.94.1) reruns the script on every build for a watched
        // path that does not exist, and neither `packed-refs` nor `../dist`
        // always does.
        if Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    // A plain `git status` (as of git 2.50.1) refreshes .git/index, which this
    // script watches, and takes index.lock while another git process may need it.
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}
