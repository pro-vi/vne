# Backlog

Lanes awaiting triage. Verdicts: architect | direct | drop | defer | pending.
Checkbox marks a lane consumed (plan written, or direct commit landed).

## Lanes

- [x] **entry-value-state** — every JSON entry says whether its key is set, empty, or a placeholder, in default and `--values` output · verdict: direct
- [x] **version-flag** — `vne --version` prints the crate version and the commit the binary was built from · verdict: direct
- [x] **key-lookup** — `vne where <KEY> [path...]` answers which env file holds a key, at which line, and whether it is set · verdict: direct · blocked by: entry-value-state
- [ ] **agent-contract-docs** — `vne help` and the README say what an agent may run on its own and what it must hand to a person · verdict: direct
- [ ] **json-stability** — the README says which JSON fields scripts may depend on · verdict: drop (a 0.x version promises nothing under semver, and the README already says 0.1.0 pre-release; reopen at 1.0)
- [ ] **named-file-read-bound** — `check` and `where` refuse a named path that is not a regular file and cap its read at the scan's size bound · verdict: defer · origin: /gate review of `vne where`, 2026-10-02
- [x] **scan-git-latency** — `where` asks git only about files that hold the key, and a finished git call is noticed within 1 ms instead of 25 ms · verdict: direct · origin: /simplify review of `vne where`, 2026-10-02
- [ ] **referenced-file-incomplete** — env files named by `package.json` or Compose that cannot be read are dropped without an incomplete record · verdict: pending · origin: /simplify review of `vne where`, 2026-10-02
- [ ] **public-release** — the GitHub repository goes public · verdict: pending · blocked by: the "Release Checklist Before Public Distribution" in `docs/THREAT_MODEL.md`
- [x] **git-status-batching** — scans run one or two git listings per directory instead of up to three commands per file · verdict: direct · origin: /gate review of the `where` latency fix, 2026-10-02
- [ ] **git-status-per-repository** — scans ask git once per repository instead of once per directory holding env files · verdict: direct · origin: /gate review of commit a2a97ed, 2026-10-02

## agent-contract-docs

Agents read `vne help`, not the README, so the contract lands in `vne help` first. What remains now that `where` and `valueState` ship: which commands an agent may run on its own, which ones need a person (`add --prompt`, `set --prompt`), and the withheld marker strings. Required for the public-release preparation.

## named-file-read-bound

Before this candidate, named paths used unbounded `fs::read_to_string`: FIFOs could block `check` or `where`, and device reads could grow without limit. Directory discovery reads were already bounded.

- Signal: none; vne is a local CLI with no error reporting.
- Candidate implementation: a shared regular-file reader caps reads at 10 MiB, refuses special files, and uses nonblocking Unix opens. Writes refuse an update larger than that read bound. Regression tests pass; the implementation is uncommitted pending review.

## referenced-file-incomplete

Env-named entries in the project directory that vne does not read are now reported as `not-regular-file`. Files named by `package.json` scripts or Compose `env_file` go through `add_discovered_env_file()`, which drops a missing file, a special file, or a symlink that leaves the project without any record.

- Candidate behavior: missing references produce nonfatal informational notices in inspection and lookup JSON/text. Unreadable and non-regular references produce incomplete records. The implementation and tests are uncommitted pending review.

## git-status-per-repository

Env files named by `package.json` or Compose can sit in many directories, and each directory costs one or two git listings. On 2026-10-02 (M5 Max, git 2.50.1), `inspect` over env files in 50 directories of one repository took 1.2 s. A /perf review measured a repository-wide classification, with one `rev-parse --show-toplevel` and repository-relative paths, at about 42 ms for both 50 and 499 directories.

- Needs a check for a nested repository between a file's directory and the top, so its files are asked in their own repository.
- Done when `inspect` over env files in 50 directories of one repository takes under 200 ms.

## public-release

The source-only candidate uses MIT and documents macOS on Apple Silicon as tested.

Before publication, complete the release checklist in `docs/THREAT_MODEL.md` and review source and history for private working records and potential credentials. Publication awaits explicit approval.
