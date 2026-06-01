# vne Threat Model

`vne` is a local desktop env-file workbench. Its safety bar is simple: opening, inspecting, and editing env files must not move secret contents outside the user's machine.

## Assets

- Env file contents, including API keys, tokens, database URLs, private keys, and webhook secrets.
- File structure around env files, including project names and local paths.
- User edits before they are saved.

## Trust Boundaries

- Rust parser and writer: trusted local code that reads and writes files selected by the user.
- Svelte UI: trusted local code that renders parsed values and sends explicit commands to the Tauri backend. Current redaction is a normal-UI presentation safeguard, not isolation from the local webview process.
- Tauri command boundary: only local UI invokes project scan, value save, and blank-key insertion.
- Native dialog: used only to pick a local directory.
- Network: not part of the product path for env contents.

## Current Controls

- Secret-like values, credential-bearing URLs/DSNs, and public-prefixed secret-looking names are redacted by default before they reach normal display fields.
- Key-shape metadata separates sensitive-looking values from browser-exposed public prefixes, so names such as `NEXT_PUBLIC_API_KEY` are not treated as safe public values.
- Findings include key names, file names, and framework load-order evidence, not raw values.
- Source context shows structural metadata such as quote style, export prefix, comments, and diagnostics without adding network or logging paths.
- Safe-edit mutation previews include only key names, file names, and the narrow edit shape.
- Advisory finding inspection changes only UI selection state; it does not mutate files.
- Layer conflict summaries name the effective file without showing competing values.
- Save operations update one selected key occurrence at a time and use atomic write with permission preservation where practical.
- Blank-key repair only appends `KEY=` for a documented missing key; it refuses duplicate or invalid keys.
- Write commands resolve the active project root and reject file paths outside that root.
- Browser preview uses sample data and cannot save or apply safe edits.
- The app has no accounts, telemetry, sync, hosted validation, or cloud calls for env data.

## Non-Goals

- `vne` is not a secrets manager.
- `vne` is not a cloud config platform.
- `vne` does not rotate, fetch, publish, or validate credentials against providers.
- `vne` does not sign releases or manage installers in this loop.

## Known Residual Risks

- Reveal mode intentionally shows secrets to the local user.
- Parsed raw values are present inside the trusted local Tauri/Svelte process in this slice; redaction hides them from normal rendering but is not a separate secret-isolation boundary.
- Raw preview intentionally shows the full selected env file after reveal mode when hidden secrets are present.
- A malicious local project can use misleading key names or comments; `vne` treats env files as data and does not execute them.
- Active project root checks reduce accidental path escape, but they are not a substitute for operating-system file permissions or user caution when opening untrusted projects.
- Screenshot or browser-inspection proof is still unavailable in this session because only navigation, not screenshot or DOM inspection, was exposed.

## Release Checklist Before Public Distribution

- Re-run source scans for network clients, telemetry SDKs, and logging of raw values.
- Re-check path-scope behavior around write commands after any command-surface change.
- Verify raw values are absent from structured logs and error paths.
- Verify macOS signing/notarization choices with explicit user approval.
- Capture screenshots using a local browser-control surface.
