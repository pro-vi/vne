# ADR 0007: The CLI Builds Without the Window, and Linux Ships Only the CLI

Output scope: how vne is built and shipped on Linux. No command's output changes.

- **Status:** Proposed
- **Date:** 2026-10-11
- **Deciders:** Provi ("go ahead, i want linux", 2026-10-11), Claude
- **Amends:** none. The CLI contract in ADR 0002 to 0005 is unchanged.

## Context

The CLI and the desktop window are one crate. `tauri`, `tauri-plugin-dialog`,
`tauri-plugin-webdriver` and `tauri-build` were unconditional dependencies, so
building any binary, `vne-cli` included, pulled in about 600 packages and, on
Linux, needed the GTK and WebKit development packages. On Ubuntu 24.04 without
them, v0.1.1 fails at `glib-sys` ("Package glib-2.0 was not found in the
pkg-config search path") after 67 s.

The CLI does not use Tauri: `cli.rs`, `git.rs` and `clipboard.rs` have no
reference to it. In `lib.rs` 16 items do, each a thin `#[tauri::command]`
wrapper over a pure `*_authorized` function, plus the window's `run` functions.

Agents often run in Linux containers, where the window cannot run anyway.

## Decision

1. Tauri and its plugins become optional dependencies behind a `desktop`
   feature that is on by default. `cargo build` and `npm run install:local`
   build what they built before.
2. `cargo build --no-default-features` builds the CLI alone: about 44 packages,
   no Node, no WebKit. `vne .` in that build exits 2 with "this build has no
   window".
3. Linux ships only that build, as a static musl binary (`scripts/release-linux.sh`),
   for aarch64 and x86_64. The window and the clipboard copy stay macOS-only.
4. `scripts/check-cli-only.sh` fails when the CLI-only dependency tree contains
   Tauri, GTK, WebKit, glib or their relatives. `scripts/validate.sh` runs it
   and the full test suite without the feature.
5. `objc2-foundation` declares the `NSEnumerator` feature that `clipboard.rs`
   uses. It compiled before only because Tauri's dependencies enabled it.

## Consequences

Positive:

- A Linux user installs one static file; a source build needs Rust only.
- A tool that handles secrets has about 44 dependencies instead of about 600 in
  the build most users run in an agent's environment.
- The Linux CLI runs 255 tests (161 + 5 + 89). The same build on macOS runs
  264, which includes 9 macOS-only tests.

Negative:

- Two feature configurations to keep green; `validate.sh` runs both.
- The library's `crate-type` still lists `cdylib`, which musl cannot build;
  cargo prints "dropping unsupported crate type `cdylib`" and builds the rest.
- The v0.1.1 tag has none of this. Linux archives start with the next release.
- Windows and Intel macOS stay unverified.

## Revisit Triggers

- The window should run on Linux or Windows.
- A dependency is needed by the CLI that cannot build on musl.
- The CLI and the window should be separate crates in a workspace (for example
  to publish the CLI alone).

## References

- `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, items marked `#[cfg(feature = "desktop")]` in `src-tauri/src/lib.rs`
- `scripts/check-cli-only.sh`, `scripts/release-linux.sh`, `scripts/test-release.mjs`
- `src-tauri/tests/cli_output.rs`, `build_without_desktop_feature_says_it_has_no_window`
