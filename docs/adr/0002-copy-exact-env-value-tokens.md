# ADR 0002: Copy Exact Env Value Tokens

Implementation scope: the metadata limitation below describes the original writer. The shared writer now preserves captured extended attributes and, on macOS, copies ACLs before replacement. See [the current metadata contract](../THREAT_MODEL.md#known-residual-risks).

- **Status:** Accepted
- **Date:** 2026-08-12
- **Deciders:** Provi, Codex

## Context

Agents need to copy one named value between env files without reading, receiving,
or printing that value. The existing parser retains quoted escape bytes, while
the existing value writer escapes caller-supplied text. Passing parsed value text
through that writer can therefore change its dotenv meaning.

The destination write also carries plaintext. A predictable temporary filename
can expose or redirect that plaintext before the final rename.

## Decision

`vne copy` transfers the source assignment's exact value token, including quote
delimiters, escape bytes, empty content, and multiline content. It excludes the
source key, `export` prefix, whitespace, and comment.

The command copies to the same key name. A missing destination key is added. An
identical value token returns `alreadyPresent` without writing. One different
destination token requires `--overwrite`; duplicate or malformed occurrences are
refused.

Copy and the existing writers use an exclusively created private temporary file
in the destination directory. The complete file is synced, assigned the
destination permissions, and atomically renamed into place. Copy output is a
dedicated receipt containing only disposition, normalized paths, and key.

## Rationale

Exact-token transfer follows the repository's byte-preserving model and avoids
inventing a dotenv decoding dialect. A decoded-value design was rejected because
the parser does not currently establish decoded semantics across dotenv variants.

A dedicated receipt prevents disclosure by construction. Redacting a full
`EnvFile` after building it would keep unnecessary payload-bearing fields in the
output path.

## Consequences

Positive:

- Copy preserves the source value's dotenv syntax without re-escaping it.
- `alreadyPresent` makes retries converge after a successful write whose output
  failed.
- Private exclusive temporary files remove the predictable-temp disclosure and
  symlink risks for all shared writer consumers.

Negative:

- Semantically equal values with different tokens still require `--overwrite`.
- Atomic replacement prevents partial files but does not lock out a
  non-cooperating concurrent writer or sync the containing directory.
- The writer preserves `std::fs::Permissions`, not inode identity, ownership
  changes, ACLs, or extended attributes.

## Revisit Triggers

- A destination-key rename use case is demonstrated. *(Fired 2026-08-19; answered by ADR 0003, which renames the key token in place instead of transferring a value token.)*
- The parser gains a specified, tested decoded dotenv value representation.
- Concurrent external writers become part of the supported contract.
- Permission preservation must include ACLs or extended attributes.

## References

- `src-tauri/src/lib.rs`
- `src-tauri/src/cli.rs`
- `src-tauri/tests/cli_output.rs`
