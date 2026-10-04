# ADR 0004: Report Git Exposure By Shelling Out To Git

- **Status:** Accepted
- **Date:** 2026-08-19
- **Updated:** 2026-10-02 — after load time became noticeable, one `ls-files`
  listing per directory replaces up to three commands per file, and a finished
  git is noticed within 1 ms instead of 25 ms. Together, over 6 env files in one
  directory, `inspect` went from 515 ms to 43 ms (M5 Max, git 2.50.1).
- **Deciders:** Provi, Claude

## Context

The worst realistic outcome for a local env file is not that `vne` prints its
contents. It is that the file gets committed. `vne` could describe an env file's
keys, shapes, and layering in detail while staying silent about the one property
that decides whether those secrets leave the machine.

## Decision

Every env file in `inspect`, `check`, and `add --json` output, and every `where`
match, carries a `gitStatus` of `tracked`, `untrackedIgnored`,
`untrackedNotIgnored`, `outsideRepository`, or `unknown`. `src-tauri/src/git.rs`
(`env_file_git_statuses`) derives it from one read-only listing per directory,
`ls-files -t --cached --others --exclude-standard`, over the files asked about
in it: a `?` tag means untracked and not ignored, and any other tag means
tracked. A file left out of that listing is ignored only if a second listing,
`ls-files --others --ignored --exclude-standard`, names it; otherwise it is
`unknown`, because a case-only rename or an uninitialized submodule also leaves
a file out (git 2.50.1). Names are passed with `--literal-pathspecs`, so a name
such as `:!x.env` is never read as pathspec magic. A name that is not ASCII gets
a listing of its own and is judged by whether anything was listed, because git
2.50.1 on macOS prints such names precomposed, which can differ from the bytes
on disk. It never reads an env file's contents and never stages, ignores, or
edits anything.

The status is computed by the filesystem-aware loaders. The pure string parser
leaves `Unknown`, because a parser handed a `String` genuinely cannot know.

`add`, `set`, `rm`, `rename`, and `copy` write one stderr warning when their
change landed in a file git already tracks; that check is one
`ls-files --error-unmatch` (`env_file_is_tracked`). The warning never blocks
the write, and a convergent no-op that wrote nothing stays quiet.

## Rationale

Shelling out matches the existing `stty` precedent and keeps the binary free of
new dependencies. `git2` or the `ignore` crate would each pull a substantial
dependency tree to answer a question the `git` binary already answers exactly,
including the ignore-precedence rules that are easy to reimplement subtly wrong.

When a listing is refused, `rev-parse --is-inside-work-tree` decides without
parsing git's English: it exits 0 in a repository whose index git cannot read,
which is `unknown`. When it refuses too, a `.git` entry at or above the
directory means git would not open a repository that is there, such as one with
dubious ownership, which is also `unknown`; only without one is the file
`outsideRepository` (git 2.50.1). `git status` would
also classify in one command, but it runs a repository's clean filter on a file
whose timestamp changed, which `ls-files` and `rev-parse` do not (both checked
on git 2.50.1).

`unknown` is a represented state rather than a hidden default. Reporting a file
as untracked because git could not be run would be the one wrong answer here.

Warning rather than refusing was chosen deliberately: committing an env file is
sometimes correct — an example file is meant to be committed — and a tool that
refuses a legitimate write teaches people to work around it.

## Consequences

Positive:

- The exposure that actually matters is visible in every file summary, and at
  the moment of a write.
- No new dependency, and no second implementation of gitignore precedence.
- The desktop payload gains the same field, so a future UI can surface it.

Negative:

- Per directory holding env files, one child process for its ASCII names, a
  second when that listing leaves some out, one or two per name that is not
  ASCII, and a `rev-parse` where a listing is refused. Env files spread over
  50 directories still took 1.2 s for `inspect` (M5 Max, git 2.50.1).
- The status is a point-in-time read that can be stale by the time a write
  happens.
- A hung `git` is killed after 5 s (`GIT_DEADLINE`) and the files it was asked
  about report `unknown`; once a scan's 10 s deadline passes, no further git
  command starts, so the one in progress can add up to 5 s.
- `tracked` means git holds the file now, not that a commit has already
  published it.
- `src/lib/types.ts` gains another hand-mirrored field with no parity guard.

## Revisit Triggers

- Load time becomes noticeable on a project whose env files spread across many
  directories, where per-directory listings add up again. *(Fired 2026-10-02:
  env files in 50 directories took 1.2 s; per-repository listing is queued in
  `docs/BACKLOG.md`.)*
- The desktop UI starts consuming `gitStatus`, which makes the hand-mirrored
  TypeScript type worth replacing with a generated one.
- Someone wants `vne` to offer to add the file to `.gitignore`, which would turn
  this read-only module into a mutating one.

## References

- `src-tauri/src/git.rs`, `src-tauri/src/lib.rs`, `src/lib/types.ts`
- `docs/THREAT_MODEL.md` (Current Controls, Known Residual Risks)
