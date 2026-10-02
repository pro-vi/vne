# Backlog

Lanes awaiting triage. Verdicts: architect | direct | drop | defer | pending.
Checkbox marks a lane consumed (plan written, or direct commit landed).

## Lanes

- [x] **entry-value-state** — every JSON entry says whether its key is set, empty, or a placeholder, in default and `--values` output · verdict: direct · origin: inbox note 2026-10-01-agent-field-notes-before-public-release.md
- [x] **version-flag** — `vne --version` prints the crate version and the commit the binary was built from · verdict: direct · origin: inbox note 2026-10-01-agent-field-notes-before-public-release.md
- [x] **key-lookup** — `vne where <KEY> [path...]` answers which env file holds a key, at which line, and whether it is set · verdict: direct · blocked by: entry-value-state · origin: inbox note 2026-10-01-agent-field-notes-before-public-release.md
- [ ] **agent-contract-docs** — `vne help` and the README say what an agent may run on its own and what it must hand to a person · verdict: defer · blocked by: key-lookup · origin: inbox note 2026-10-01-agent-field-notes-before-public-release.md
- [ ] **json-stability** — the README says which JSON fields scripts may depend on · verdict: drop (a 0.x version promises nothing under semver, and the README already says 0.1.0 pre-release; reopen at 1.0) · origin: inbox note 2026-10-01-agent-field-notes-before-public-release.md
- [ ] **named-file-read-bound** — `check` and `where` refuse a named path that is not a regular file and cap its read at the scan's size bound · verdict: defer · origin: /gate review of `vne where`, 2026-10-02
- [ ] **scan-git-latency** — `where`, `inspect`, and desktop scans spend most of their time waiting on git checks · verdict: direct · origin: /simplify review of `vne where`, 2026-10-02
- [ ] **referenced-file-incomplete** — env files named by `package.json` or Compose that cannot be read are dropped without an incomplete record · verdict: pending · origin: /simplify review of `vne where`, 2026-10-02

## agent-contract-docs

Agents read `vne help`, not the README, so the contract lands in `vne help` first. What remains after key-lookup and entry-value-state ship: which commands an agent may run on its own, which ones need a person (`add --prompt`, `set --prompt`), and the withheld placeholder strings. Promote when the public-release decision is made.

## named-file-read-bound

A path named on the command line is read with a plain `fs::read_to_string`: a FIFO blocks `check` or `where` until killed, and `/dev/zero` grows memory without limit (4.3 GB in about a second). Directory scans already refuse non-regular files and bound each read with `read_discovery_file_bounded()`.

- Signal: none; vne is a local CLI with no error reporting.
- Promote when: one report of `check` or `where` hanging or exhausting memory on a named path.

## scan-git-latency

Measured on 2026-10-02 over a project with 6 env files: `vne where` with a missing key took 520 ms with git on PATH and 10 ms without. The scan checks git status for every discovered file, and `git.rs` sleeps 25 ms after each git spawn before polling it. Past about 110 files, git time alone pushes files past the 10 s scan deadline as `scan-timeout`.

- Options: `where` asks git only for files that hold the key; the poll in `git.rs` backs off from about 1 ms instead of sleeping 25 ms.
- Done when a `where` miss over the same 6 files takes under 100 ms with git on PATH.

## referenced-file-incomplete

Env-named entries in the project directory that vne does not read are now reported as `not-regular-file`. Files named by `package.json` scripts or Compose `env_file` go through `add_discovered_env_file()`, which drops a missing file, a special file, or a symlink that leaves the project without any record.

- Owner decision: reporting them would change `inspect`'s exit code for every project whose `package.json` names an env file that does not exist on this machine, which is common for `.env.production`.
