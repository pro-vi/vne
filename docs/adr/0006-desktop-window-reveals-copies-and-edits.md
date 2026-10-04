# ADR 0006: The Desktop Window Reveals, Copies and Edits; Everything Else Stays in the CLI

Output scope: the desktop window shows a stored secret only while the user holds Option, and copies one value to the macOS clipboard on request. See [the current threat model](../THREAT_MODEL.md#known-residual-risks).

- **Status:** Accepted
- **Date:** 2026-10-04
- **Deciders:** Provi, Claude
- **Amends:** ADR 0001 (its drafts, duplicate-save confirmations and raw preview are gone; target-bound entry refs, generation-guarded operations and fail-closed input masking remain)

## Context

The window had grown a findings drawer, a key filter, a raw file preview, a
duplicate-save confirmation and a file-naming dialog. Meanwhile the CLI came to
cover inspection and structural edits for terminals and agents: `inspect`,
`check`, `where`, `rm`, `rename`, `copy`, `example`.

A terminal session an agent runs can end up in the agent's transcript, so the
CLI never prints a stored secret by default. The window is a separate process
the agent cannot see. That makes it the place for the acts in which a person
has to see or move a secret value, and for little else.

## Decision

The window does three jobs.

1. **Reveal.** A stored secret shows only while Option is held, and hides on
   release, when the window loses focus, or when the selection changes.
2. **Copy.** Cmd-C copies the selected value through the backend command
   `copy_env_value`; the value never enters the webview. A value the shape
   check calls sensitive or redacts by default is written for this Mac only,
   carries the `org.nspasteboard.ConcealedType` and `TransientType` markers,
   and is cleared after 30 s, when the app exits, and on SIGINT, SIGTERM or
   SIGHUP (`exit_on_termination_signals`), as long as the clipboard still
   holds that value. "Still holds" means the pasteboard change count is
   unchanged or its text matches a keyed fingerprint taken at copy time
   (`clipboard::clear_concealed`); the value itself is not kept.
3. **Edit.** Enter edits the selected row and Enter saves it, one write per
   row. A row that needs a value (empty, a placeholder, or listed in the
   example file but missing from `.env`) opens the next such row after it is
   saved or skipped (`nextNeedingIndex` in `src/lib/rows.ts`). Enter on an
   empty field writes nothing. Cmd-N adds a key. A project with a root example
   file and no `.env` can create `.env` as a byte copy of the example
   (`create_env_from_example_authorized`).

Removing or renaming keys, copying a key between files, the findings review,
filtering and the raw preview are not in the window. A duplicated key shows the
`vne rm` command that keeps the selected line, in line with ADR 0003.

## Rationale

- **One edit surface, not a fill form.** A separate "needs a value" form with
  one save was drawn next to the chosen design. It would have been a second
  place that edits values, needed a new backend write for several keys at once
  that could stop halfway, and held every typed secret unsaved in the webview.
  Saving per row reuses the existing `save_env_value` and `add_env_key`.
- **No activity log.** A log of writes cannot keep old values without becoming
  a second copy of the secrets, so it cannot restore anything. The agent's
  transcript already records the commands it ran, and a log would miss edits
  made with any tool other than vne.
- **No multi-project sweep.** It would widen the one-folder `AuthorizedRoot`
  permission model for a need nobody had observed.
- **Option, not Tab, for reveal.** Tab means "next field" in every form, so a
  user moving on would put a secret on screen. Holding a key means letting go
  hides it.
- **Clear by value, not by change count alone.** Clipboard tools can re-post
  an item as plain text without its markers. That changes the change count
  while the secret is still on the clipboard; a count-only rule would then
  leave it there indefinitely.

## Consequences

Positive:

- A stored secret reaches the webview only while Option is held.
- Filling a project is typing and Enter, row after row, with nothing left unsaved.
- The window carries less code than before: it lost the drawer, filter, raw
  preview, confirmation and dialog, and the helpers only they used.

Negative:

- Nothing lets the user look over several values before any is written.
- Copy is macOS-only; other platforms return an error.
- A copied secret is readable by any same-user process for up to 30 s, and a
  crash or SIGKILL skips the clear (threat model, Known Residual Risks).
- The window cannot remove or rename keys; the user runs the CLI command it shows.
- `EXAMPLE_ENV_FILE_NAMES` exists in both `src-tauri/src/lib.rs` and
  `src/lib/rows.ts` and must be kept in step.

## Revisit Triggers

- Users need to remove or rename keys from the window often enough to justify
  it (ADR 0003's trigger for the same move).
- The desktop app ships on a platform other than macOS.
- Users ask to review all values before any is written.
- A common clipboard tool is found to record items marked concealed.

## References

- `src/App.svelte`, `src/lib/rows.ts`, `src-tauri/src/clipboard.rs`
- `copy_env_value`, `create_env_from_example_authorized` and `exit_on_termination_signals` in `src-tauri/src/lib.rs`
- ADR 0001 (target-bound workbench state), ADR 0003 (refuse what file state cannot prove)
