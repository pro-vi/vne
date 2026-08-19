# ADR 0003: Refuse What File State Cannot Prove

- **Status:** Accepted
- **Date:** 2026-08-19
- **Deciders:** Provi, Claude
- **Supersedes:** nothing; extends ADR 0002 to the full key lifecycle

## Context

`vne` could add a key, copy one between files, and read files. It could not
change a value, delete a key, rename a key, or refresh an example file. Anyone
needing those gestures fell back to a text editor, which is exactly where a
secret value ends up on screen, in a scrollback buffer, or in a transcript.

Filling the gap raises a question the existing verbs mostly dodged: what should
a command do when it is asked to repeat itself? An agent that loses a receipt
reruns the command. For `add` and `copy` the answer was already `alreadyPresent`
— the file proves the intended end state was reached. For a rename or a
line-numbered delete, the file proves nothing of the kind.

## Decision

`set`, `rm`, `rename`, and `example` ship as CLI-only verbs in the existing verb
anatomy: one `Command` variant, one parser, one core function in `lib.rs`, one
`atomic_write_preserving_permissions` call, one payload-free receipt struct. No
new Tauri commands, matching the deliberate `copy` precedent.

Each verb converges when, and only when, the file proves the end state:

- `set` reports `alreadyPresent` when the stored parsed value equals the
  supplied one, and does not write.
- `rm` reports `alreadyAbsent` when the key is gone, and does not write.
- `example` reports `alreadyCurrent` when no key is missing, and does not write.
- `rename` has no convergent state. When the old key is absent and the new key
  is present, it refuses as `Indeterminate` rather than claiming a rename it
  cannot attribute to itself.

Where a selector could act on the wrong entry, the verb fails closed. `rm --line
<N>` removes the occurrence at line N only if line N still holds that key; it
never searches for another occurrence. `rm` refuses an occurrence the parser
flagged as malformed, because an unclosed quote makes an entry's extent run to
end of file. `--expect present|absent` lets a caller turn a convergent no-op
into exit 2 when convergence would hide a wrong belief.

`set` accepts its value only from stdin or a hidden prompt. No argument form
exists, and `add` warns when a secret-looking value arrives through one. `set`
also refuses an empty or whitespace value unless `--allow-empty` says so: a
pipeline that produced nothing is indistinguishable from a deliberate clear, and
the difference is somebody's stored secret.

No message echoes the value half of an argument. A caller who pastes
`KEY=secret` where a key name belongs gets `KEY=…` and a sentence naming the
mistake; `rm` and `rename` refuse such an argument at parse time, as `set`
already did. This holds for rejected keys, unknown options, the secret-like
argument warning, and the hidden prompt's label, because stderr is what shell
history, CI logs, and agent transcripts keep.

Receipts keep their convergence states, and the README, `vne help`, and
`docs/THREAT_MODEL.md` state the resulting claim scope: values never appear in
vne's output, but a disposition can reveal that a supplied value equals the
stored one.

## Rationale

Every mechanism these verbs need already existed with tests and a documented
safety rationale. A generic "edit engine" unifying set, remove, and rename
behind one transform API was rejected: their failure taxonomies are disjoint —
stale selectors, rename provenance, duplicate discipline — and the risk lives in
those per-verb contracts, not in shared editing code.

Reporting `alreadyRenamed` was rejected because it would be a claim about
history derived from a snapshot of state. The same reasoning drives the stale
selector rule: a line number copied from earlier output is a claim about a file
that may have moved on. In both cases the honest answer is a refusal that names
what was actually found.

Collapsing the convergence states to hide value equality was rejected. It would
cost every retry-safety property the receipts provide in exchange for hiding one
bit that any local process can obtain by reading the file, and it would make the
guarantee vaguer rather than sharper. Stating the scope is stronger than
narrowing the output.

`rename` transfers nothing: it rewrites the key token in place. The alternative
considered during planning — splice the old line out and append `NEW=<value
token>` — would have dropped the line's inline comment and its `export` prefix,
and `export` changes what a shell does with the variable.

## Consequences

Positive:

- The whole key lifecycle is reachable without opening an env file in an editor.
- A lost receipt is recoverable by rerunning the command, except where rerunning
  could destroy something, where it fails instead.
- Every new error path names keys, line numbers, and paths only; the sentinel
  harness asserts this over stdout and stderr on every success and failure path.
- `rename` produces a diff limited to the key name.

Negative:

- Dispositions disclose value equality. This is stated rather than removed.
- Two occurrences of one key on adjacent lines defeat the `rm --line` guard: after
  the first is removed the second occupies that line, and an identical retry
  deletes it. File state cannot distinguish that retry from a fresh intent.
- A malformed entry cannot be removed with `vne rm`; the quoting must be fixed by
  hand first.
- `rm` and `rename` are CLI-only, so the desktop app still cannot delete or rename
  a key.
- `example` never prunes, so a key retired from the real file lingers in the
  example file until someone removes it.

## Revisit Triggers

- A caller needs to remove a malformed entry and has no editor available.
- Storing an empty value becomes common enough that `--allow-empty` is friction
  rather than a guard.
- The desktop app needs delete or rename, which would move these cores behind
  Tauri commands.
- An occurrence count on the `rm` receipt is wanted so a surviving duplicate is
  visible at the moment of mutation.
- `--expect` is wanted on verbs other than `rm`.
- Anyone argues from the receipts that vne promises value indistinguishability.

## References

- `docs/plans/2026-08-18-001-feat-cli-secret-gesture-verbs-plan.md`
- `docs/adr/0002-copy-exact-env-value-tokens.md` (its rename revisit trigger)
- `src-tauri/src/lib.rs`, `src-tauri/src/cli.rs`, `src-tauri/tests/cli_output.rs`
- `docs/THREAT_MODEL.md` (Known Residual Risks)
