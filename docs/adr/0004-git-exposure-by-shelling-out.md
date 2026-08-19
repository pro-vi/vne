# ADR 0004: Report Git Exposure By Shelling Out To Git

- **Status:** Accepted
- **Date:** 2026-08-19
- **Deciders:** Provi, Claude

## Context

The worst realistic outcome for a local env file is not that `vne` prints its
contents. It is that the file gets committed. `vne` could describe an env file's
keys, shapes, and layering in detail while staying silent about the one property
that decides whether those secrets leave the machine.

## Decision

Every env file in `inspect` and `check` output carries a `gitStatus` of
`tracked`, `untrackedIgnored`, `untrackedNotIgnored`, `outsideRepository`, or
`unknown`. A new `src-tauri/src/git.rs` derives it by running read-only git
plumbing in the file's directory: `rev-parse --is-inside-work-tree`, then
`ls-files --error-unmatch`, then `check-ignore -q`. It never reads the env
file's contents and never stages, ignores, or edits anything.

The status is computed by the filesystem-aware loaders. The pure string parser
leaves `Unknown`, because a parser handed a `String` genuinely cannot know.

`add`, `set`, `rm`, `rename`, and `copy` write one stderr warning when their
change landed in a file git already tracks. The warning never blocks the write,
and a convergent no-op that wrote nothing stays quiet.

## Rationale

Shelling out matches the existing `stty` precedent and keeps the binary free of
new dependencies. `git2` or the `ignore` crate would each pull a substantial
dependency tree to answer a question the `git` binary already answers exactly,
including the ignore-precedence rules that are easy to reimplement subtly wrong.

Three commands rather than the two originally budgeted: telling
"outside any repository" apart from "inside a repository but untracked" without
parsing git's English needs the `rev-parse` probe first. All three are
index-only plumbing, run once per file at load time.

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

- Up to three child processes per env file at load time.
- The status is a point-in-time read that can be stale by the time a write
  happens.
- A hung `git` hangs the command; there is no timeout.
- `tracked` means git holds the file now, not that a commit has already
  published it.
- `src/lib/types.ts` gains another hand-mirrored field with no parity guard.

## Revisit Triggers

- Load time becomes noticeable on a project with many env files.
- A hung or pathological `git` is observed, making a timeout necessary.
- The desktop UI starts consuming `gitStatus`, which makes the hand-mirrored
  TypeScript type worth replacing with a generated one.
- Someone wants `vne` to offer to add the file to `.gitignore`, which would turn
  this read-only module into a mutating one.

## References

- `docs/plans/2026-08-18-001-feat-cli-secret-gesture-verbs-plan.md`
- `src-tauri/src/git.rs`, `src-tauri/src/lib.rs`, `src/lib/types.ts`
- `docs/THREAT_MODEL.md` (Current Controls, Known Residual Risks)
