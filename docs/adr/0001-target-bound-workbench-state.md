# ADR 0001: Use Target-Bound Workbench State

- **Status:** Accepted
- **Date:** 2026-07-28
- **Deciders:** Provi, Codex

## Context

The desktop workbench previously represented selection, revealed plaintext,
edit values, duplicate confirmation, and loading state as loosely related
component scalars. In legacy Svelte, helpers also hid some dependencies from
the compiler. Ordinary updates could remain stale, and an asynchronous result
could be associated with a different live selection.

The Rust commands already identify one env occurrence exactly and return safe,
redacted snapshots. The frontend needs to preserve that identity and privacy
contract across asynchronous UI work without changing the command API.

## Decision

Keep the existing legacy Svelte component style, but give workbench domain
state a pure TypeScript owner:

- represent an entry by root, file path, entry id, key, and line number;
- store revealed plaintext and edit drafts together with that exact target;
- serialize snapshot-changing actions through a generation-bearing pending
  operation and accept a targeted result only for the current target;
- pass state explicitly into legacy reactive and template helpers;
- derive input protection from Rust-produced key-shape metadata, failing closed
  for unknown or conflicting keys;
- keep human-facing notice text separate from operation authority.

## Rationale

This retains the repository's thin-component plus pure-`src/lib` pattern and
fits its existing Vitest setup. A whole-component Svelte 5 runes migration
would reduce the chance of hidden dependencies, but would combine a broad
syntax and lifecycle migration with a privacy-sensitive correctness repair.

## Consequences

Positive:

- Plaintext, drafts, saves, and duplicate confirmations cannot drift between
  same-looking occurrences.
- State rules are testable without a browser or new dependencies.
- The Tauri command and serialized snapshot contracts remain unchanged.

Negative:

- Snapshot-changing UI work is intentionally serialized.
- Legacy Svelte call sites must keep reactive dependencies lexically explicit.
- Raw preview stays conservatively unavailable for any file containing a
  Rust-classified protected entry until the backend exposes explicit raw
  availability metadata.

## Revisit Triggers

- `App.svelte` is migrated to Svelte 5 runes as a focused change.
- Operations need safe concurrency rather than short serialized mutations.
- The backend adds versioned snapshots, cancellation, or explicit raw-content
  availability metadata.

## References

- `src/lib/workbench-state.ts`
- `src/lib/workbench-state.test.ts`
- `docs/THREAT_MODEL.md`
