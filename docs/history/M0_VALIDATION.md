# M0 validation — 20 October 2026

Scope: Lyra toolchain, mock UI/player, typed IPC, application assets, native packaging, and automated checks. M1–M5 remain unimplemented. The starter was imported from `Downloads/rust-dsd-player` into `/home/bearofjupiter/dev/lyra`, then revised after an independent review (see "Review fixes" below).

## Environment

- Linux Mint 22.3 (Ubuntu noble), x86_64; kernel `7.2.6-070206-generic`.
- Rust/Cargo 1.99.1 via rustup in `~/.cargo`, pinned by `rust-toolchain.toml`; edition 2024 in all six workspace members.
- Node 24.22.0 LTS via nvm; npm 11.19.1.
- GTK 3.24.41; WebKitGTK 2.52.6; ALSA 1.2.11; librsvg 2.58.0; Ayatana AppIndicator 0.5.90.
- Tauri Rust/API 2.12.4; CLI 2.12.3; Svelte 5.57.2; Vite 8.3.4; TypeScript 6.0.3; Vitest 5.0.5. Exact versions are in the lockfiles.
- No DAC was opened. DAC model/revision/firmware, negotiated audio formats, and audio integrity are not applicable to M0.

## Checks (final M0 state, persistent toolchains, `CARGO_BUILD_JOBS=2`)

| Command | Result |
| --- | --- |
| `scripts/check.sh` | PASS end to end |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS with the workspace lint policy (pedantic; `unwrap`/`expect`/`panic` denied outside tests) |
| `cargo test --workspace --locked` | PASS: 1 test (`get_app_info` dispatched through Tauri's command handler with the mock runtime). The five engine/library crates are empty and have no tests. |
| `cargo deny --locked check` | PASS: advisories, bans, licenses, sources. One documented advisory exception (RUSTSEC-2024-0370, from Tauri's gtk-rs 0.18). Duplicate-version warnings come from Tauri's tree. |
| `cargo audit` (0.22.2) | PASS: 0 vulnerabilities; 2 warnings (RUSTSEC-2024-0370 unmaintained, RUSTSEC-2024-0429 unsound), both from gtk-rs 0.18 pinned by Tauri; not reachable from Lyra code |
| `npm ci` / `npm audit` | PASS: zero reported vulnerabilities |
| `npm run format:check` | PASS |
| `npm run check` | PASS: zero errors, zero warnings |
| `npm run lint` | PASS: ESLint (typescript-eslint strictTypeChecked + stylisticTypeChecked, eslint-plugin-svelte, anti-slop), zero warnings |
| `npm test` | PASS: 9 IPC boundary tests |
| `npm run build` | PASS: production frontend built with Vite 8 |
| `npm run tauri build -- --debug --bundles deb` | PASS: `target/debug/bundle/deb/Lyra_0.1.0_amd64.deb` (19 MiB) |
| `desktop-file-validate …/data/usr/share/applications/*.desktop` | PASS |
| `xvfb-run -a npm run test:desktop` (tauri-driver 2.0.5) | PASS: real WebKitGTK window, 3 mock albums, disabled player, initial and retried Rust IPC (`Lyra 0.1.0 · Rust connected · linux`) |
| `xvfb-run -a npm run test:dev` | PASS: `tauri dev` built and opened the Lyra window |
| Remote CI run | NOT RUN: nothing pushed yet |

The IPC boundary tests inject a fake command runner (no module mocking) and cover a valid response, browser preview, four malformed responses, and Tauri rejections with a string, an error object, and anything else.

## Review fixes

- Naming: "Lyra" in all user-facing text, `lyra` in identifiers. Removed "Auralis" and "Codex" references.
- Removed tautological placeholder tests and the dead `ImplementationStatus` enums; the five future crates are now empty with a one-line description.
- Removed the unused `AppError` envelope and the `thiserror` dependency; `run()` returns `tauri::Result`. The IPC error format moves to M1 with the first fallible command.
- Removed `audioEngine` from `get_app_info`; the frontend checks shape only and builds a typed value without a cast.
- Removed mobile/Windows scaffold leftovers (`staticlib`/`cdylib` crate types, `mobile_entry_point`, `windows_subsystem`, `.icns`/`.ico` bundle icons).
- Pinned Rust 1.99.1; app version now comes from Cargo only.
- Upgraded Vite 6→8, vite-plugin-svelte 5→7, prettier-plugin-svelte 3→4, TypeScript 5.9→6.0 (TypeScript 7 is not yet supported by svelte-check). Added `vite-env.d.ts`.
- UI: sidebar items are real buttons; grid layout instead of a hard-coded footer height; global CSS moved to `src/app.css`; backend status visible without scrolling.
- CI: runs on `main` pushes and PRs, installs the pinned toolchain, caches Rust builds, 45-minute timeout.
- Spec: engine uses threads and channels, no async runtime; PipeWire holding the DAC is treated as the common case.
- Tooling: workspace lint policy and `clippy.toml`; release/profiling build profiles (dependencies optimized and without debug info in dev); `deny.toml`; `.editorconfig`; ESLint with vendored anti-slop rules; stricter `tsconfig.json`; IPC errors are `BackendError` instances parsed at the boundary.
- Docs and layout: short README plus `docs/DEVELOPMENT.md`; cleaner `.gitignore`; smoke tests moved to `tests/e2e/`; six vendored Rust skills in `.claude/skills/`; VS Code settings that give rust-analyzer its own target directory.

## Notes

The first native build attempts failed because GLib/GTK/WebKit development packages were missing; after they were installed, native validation succeeded. The [desktop screenshot](../../artifacts/desktop.png), `artifacts/desktop-smoke.json`, and logs are local generated artifacts and are not committed. No playback, parsing, device discovery, scanner, DSP, or bit-perfect functionality is implemented or claimed.
