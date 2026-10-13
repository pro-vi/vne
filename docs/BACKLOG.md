# Backlog

Lanes awaiting triage. Verdicts: architect | direct | drop | defer | pending.
Checkbox marks a lane consumed (plan written, or direct commit landed).

## Lanes

- [x] **entry-value-state** — every JSON entry says whether its key is set, empty, or a placeholder, in default and `--values` output · verdict: direct
- [x] **version-flag** — `vne --version` prints the crate version and the commit the binary was built from · verdict: direct
- [x] **key-lookup** — `vne where <KEY> [path...]` answers which env file holds a key, at which line, and whether it is set · verdict: direct · blocked by: entry-value-state
- [x] **agent-contract-docs** — `vne help` and the README say what an agent may run on its own and what it must hand to a person · verdict: direct
- [ ] **json-stability** — the README says which JSON fields scripts may depend on · verdict: drop (a 0.x version promises nothing under semver, and the README already says 0.1.0 pre-release; reopen at 1.0)
- [x] **named-file-read-bound** — `check` and `where` refuse a named path that is not a regular file and cap its read at the scan's size bound · verdict: direct · origin: /gate review of `vne where`, 2026-10-02
- [x] **scan-git-latency** — `where` asks git only about files that hold the key, and a finished git call is noticed within 1 ms instead of 25 ms · verdict: direct · origin: /simplify review of `vne where`, 2026-10-02
- [x] **referenced-file-incomplete** — env files named by `package.json` or Compose produce notices when missing and incomplete records when unreadable · verdict: direct · origin: /simplify review of `vne where`, 2026-10-02
- [x] **public-release** — the GitHub repository is public with MIT-licensed source · verdict: direct
- [x] **git-status-batching** — scans run one or two git listings per directory instead of up to three commands per file · verdict: direct · origin: /gate review of the `where` latency fix, 2026-10-02
- [ ] **git-status-per-repository** — scans ask git once per repository instead of once per directory holding env files · verdict: direct · origin: /gate review of commit [5a36983](https://github.com/pro-vi/vne/commit/5a3698360f8e95b86d48622d65d208cbe3e0157c), 2026-10-02
- [ ] **linux-ci** — every push builds the CLI without the window and runs its tests on Linux x86_64 and arm64 · verdict: pending · origin: launch-day check by hand, 2026-10-12

## agent-contract-docs

Shipped in [260a3a6](https://github.com/pro-vi/vne/commit/260a3a6fe3f58a09001eb929e11ac9d9524c5f83). CLI help and the README identify commands agents may run autonomously and value-entry operations to hand to a person. Inspection and lookup withhold stored values by default; human value entry uses local terminal prompts.

## named-file-read-bound

Shipped in [260a3a6](https://github.com/pro-vi/vne/commit/260a3a6fe3f58a09001eb929e11ac9d9524c5f83). Named-file reads use a shared regular-file reader with a 10 MiB limit. Unix opens are nonblocking, and special files are refused. Writes refuse updates larger than the read limit. Regression tests cover FIFO refusal, size bounds and unchanged regular-file behavior.

## referenced-file-incomplete

Shipped in [260a3a6](https://github.com/pro-vi/vne/commit/260a3a6fe3f58a09001eb929e11ac9d9524c5f83). Missing package or Compose env references produce informational notices in inspection and lookup JSON/text without changing exit codes. Present unreadable or non-regular references produce incomplete records. References outside the project scope remain excluded.

## git-status-per-repository

Env files named by `package.json` or Compose can sit in many directories, and each directory costs one or two git listings. On 2026-10-02 (M5 Max, git 2.50.1), `inspect` over env files in 50 directories of one repository took 1.2 s. A /perf review measured a repository-wide classification, with one `rev-parse --show-toplevel` and repository-relative paths, at about 42 ms for both 50 and 499 directories.

- Needs a check for a nested repository between a file's directory and the top, so its files are asked in their own repository.
- Done when `inspect` over env files in 50 directories of one repository takes under 200 ms.

## linux-ci

The repository has no CI. The Linux install line in the README was checked by hand on 2026-10-11: `cargo install --git https://github.com/pro-vi/vne --locked --no-default-features --bin vne` installed on macOS and on Ubuntu 24.04 arm64, `vne --version` printed, and `vne .` exited 2 with "this build has no window". x86_64 hardware was not checked. A later push can break that line without anyone noticing.

- Done when a push that breaks the install line above, or the CLI-only tests (`npm run test:cli-only`), turns a check red on Linux before anyone installs it.

## public-release

The repository is public at [pro-vi/vne](https://github.com/pro-vi/vne), with source-only distribution under MIT. macOS on Apple Silicon is documented as the tested platform. The Linux CLI build is described in ADR 0007.

The initial public source is [260a3a6](https://github.com/pro-vi/vne/commit/260a3a6fe3f58a09001eb929e11ac9d9524c5f83). An anonymous clone verified the reviewed source tree and preserved commit dates. Private working records are excluded from the public history.
