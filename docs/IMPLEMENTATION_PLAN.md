# Lyra implementation plan

Implement in order. **Do not mark a milestone complete without its acceptance checks.** Keep the project buildable at each stage.

## M0 — Toolchain and UI shell

- [x] Verify stable Rust, Cargo, Node LTS, Tauri 2 and WebKitGTK dependencies on Mint 22.3.
- [x] Make `cargo test --workspace`, `npm run check`, `npm run build`, and `npm run tauri dev` pass.
- [x] Generate Tauri icons and Linux desktop metadata.
- [x] Create CI for Rust formatting, Clippy, tests, frontend checks and builds.
- [x] Add typed Tauri IPC (`get_app_info`). The IPC error format is designed together with the first command that can fail (M1); M0 has none.

**Done when:** desktop opens, displays mock library/player, and IPC `get_app_info` responds.

Validated on Linux Mint 22.3 on 20 October 2026. All acceptance checks pass; see [M0 validation report](M0_VALIDATION.md). CI is configured; a remote CI run is not claimed.

## M1 — ALSA discovery and PCM playback

- [x] Implement ALSA device listing and capability probing in `audio-alsa`.
- [x] Implement an output abstraction (`AudioSink`) and a mock sink.
- [x] Decode WAV/FLAC via Symphonia (verify supported features). Integer PCM only; float WAV and >2 channels are refused.
- [x] Build playback controller, bounded buffering, stop/pause/seek, error handling.
- [x] Define the IPC error format (code + message) used by fallible commands. `IpcError { code, message }` in `src-tauri/src/ipc.rs`, mirrored in `src/lib/ipc.ts`.
- [x] Handle a DAC held by PipeWire/WirePlumber: clear "device busy" error naming the holder where possible; document how to release the device.
- [x] Verify direct `hw:` PCM output, rate changes, and no implicit resampling.

**Done when:** 16/44.1 and 24/96 test files play bit-perfectly through the iFi ZEN ONE (`hw:CARD=ONE,DEV=0`), with the result recorded in `docs/hardware/ifi-zen-one.md`.

Hardware results (exact format and rate, busy device, unplug, DAC volume at 0 dB) are in the [ZEN ONE report](hardware/ifi-zen-one.md). The desktop app plays dropped FLAC/WAV files on a chosen `hw:` device.

## M2 — DSF, DFF, DoP

- [x] Implement validated DSF/DFF readers with bounded allocations.
- [x] Add golden fixtures for stereo, block boundaries, seek, truncated/corrupt files (`scripts/make-dsd-fixtures.py`).
- [x] Implement and unit-test DoP framing and endian handling.
- [x] Implement DoP device negotiation, output policy and diagnostics. DoP only at 0 dB USB volume; channel counts the DAC lacks are refused.
- [ ] Validate DSD64/128 on physical DAC. DSD64 plays on the ZEN ONE (176.4 kHz carrier); DSD128 and the DAC indicator pending.

**Done when:** test fixtures verify exact DSD bytes and DAC recognizes DoP.

## M3 — Native DSD

- [ ] Probe ALSA native DSD hardware formats.
- [ ] Implement format-specific packing with exact ordering tests.
- [ ] Detect native/DoP availability and select according to explicit policy.
- [ ] Test DSD64/128/256 subject to real hardware support; record evidence.

**Done when:** native mode is proven where supported; unsupported modes fail clearly.

## M4 — Library and polished UI

- [ ] SQLite migrations, folder scanner, metadata/artwork cache.
- [ ] Album/artist/track browser, queue, playlist editing, Now Playing.
- [ ] Settings, diagnostics, dark design system, accessibility and keyboard shortcuts.
- [ ] Incremental scan and error states.

**Done when:** 10k-track scan remains responsive during playback.

## M5 — Production hardening

- [ ] Gapless and seek regression tests; unplug/replug tests; buffer underrun diagnostics.
- [ ] MPRIS integration and icons.
- [ ] Take the DAC from PipeWire cleanly: request it through the device-reservation D-Bus protocol (`org.freedesktop.ReserveDevice1`), which PipeWire/WirePlumber honor, and hand it back when Lyra releases it. One D-Bus client (`zbus`) serves this and MPRIS.
- [ ] Packages for recent distributions: `.deb`, `.rpm`, AppImage, Flatpak (needs explicit ALSA device access in the sandbox), AUR. Test each on a real install.
- [ ] Parser fuzzing, dependency license/security audit, log privacy review.
- [ ] Release notes, installation instructions, hardware compatibility report.

**Done when:** acceptance tests A01–A12 are documented with pass/fail evidence.

## Initial M0 prompt

> Read AGENTS.md, docs/SPECIFICATION.md, and docs/IMPLEMENTATION_PLAN.md. Implement milestone M0 only. Verify the Rust workspace and Svelte/Tauri app compile, install or document missing Linux prerequisites, add minimal IPC get_app_info, generate icons, and add CI. Do not implement audio playback yet. Run all required checks, report commands and results, and update milestone checkboxes only when validated.
