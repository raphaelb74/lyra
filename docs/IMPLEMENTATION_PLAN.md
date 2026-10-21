# Lyra implementation plan

Implement in order. **Do not mark a milestone complete without its acceptance checks.** Keep the project buildable at each stage.

## M0 — Toolchain and UI shell

- [x] Verify stable Rust, Cargo, Node LTS, Tauri 2 and WebKitGTK dependencies on Mint 22.3.
- [x] Make `cargo test --workspace`, `npm run check`, `npm run build`, and `npm run tauri dev` pass.
- [x] Generate Tauri icons and Linux desktop metadata.
- [x] Create CI for Rust formatting, Clippy, tests, frontend checks and builds.
- [x] Add typed Tauri IPC (`get_app_info`). The IPC error format is designed together with the first command that can fail (M1); M0 has none.

**Done when:** desktop opens, displays mock library/player, and IPC `get_app_info` responds.

Validated on Linux Mint 22.3 on 20 October 2026. All acceptance checks pass; see [M0 validation report](history/M0_VALIDATION.md). CI is configured; a remote CI run is not claimed.

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
- [x] Validate DSD64/128 on physical DAC. ZEN ONE: DSD64 at 176.4 kHz and DSD128 at 352.8 kHz carriers; the DAC indicates DSD.

**Done when:** test fixtures verify exact DSD bytes and DAC recognizes DoP.

Done 2026-10-20: fixtures and engine tests verify every DSD bit through DoP; results in the [ZEN ONE report](hardware/ifi-zen-one.md#dop-m2).

## M3 — Native DSD

- [x] Probe ALSA native DSD hardware formats.
- [x] Implement format-specific packing with exact ordering tests (`DSD_U8`, `DSD_U16_LE/BE`, `DSD_U32_LE/BE`).
- [x] Detect native/DoP availability and select according to explicit policy (`Auto`: native then DoP; `Native`; `DoP`; never PCM).
- [x] Test DSD64/128/256 subject to real hardware support; record evidence ([ZEN ONE](hardware/ifi-zen-one.md#native-dsd-m3)).

**Done when:** native mode is proven where supported; unsupported modes fail clearly.

## M4 — Library and polished UI

Five PRs: **M4a** library and album browser, **M4b-1** queue, gapless and Now Playing, **M4b-2** playlists, Artists and Tracks views, **M4c-1** Settings and DSD→PCM conversion, **M4c-2** diagnostics and taking the DAC from PipeWire.

- [x] SQLite migrations, folder scanner, metadata/artwork cache (M4a: `crates/library`; bundled SQLite, Symphonia tags for FLAC/WAV, `id3` for DSF/DFF; folders added by drop or a folder picker opened from Rust).
- [x] DSD→PCM conversion as the last resort, with its Settings switch (M4c-1: `crates/dsd-pcm`; 88.2 kHz output, 44.1 kHz fallback; see the [design](DSD_TO_PCM_DESIGN.md) for the measured choices): native DSD, then DoP, then a low-pass/decimating conversion to 24-bit PCM at the highest 44.1 kHz-family rate the device takes, level-matched. Always labelled "not bit-perfect"; runs on the controller thread, never the writer. Settings: "DSD on devices without DSD support: Convert to PCM / Refuse" and "DSD transport: Auto / Native / DoP". Updates the spec non-goal to "no *silent* DSD→PCM conversion". Tests: synthetic DSD tone keeps frequency and level, no clipping.
- [ ] Album/artist/track browser, queue, playlist editing, Now Playing. (M4a: album grid and album tracks. M4b-1: engine queue with gapless continuation of same-format tracks, DoP marker phase kept across boundaries, next/previous/skip, Now Playing and Queue views. M4b-2: playlists (migration 2; entries keep their own id, follow their track out of the library; create, rename, delete, add, move, remove), Artists and Tracks views with search.)
- [x] Settings and diagnostics (M4c). M4c-1: Settings view (DSD transport, conversion, conversion gain, pause before playback on a format change) saved to `settings.json`; hardware volume control that follows the desktop's volume keys, with the bit-perfect verdict updated live and DoP stopped if the volume moves; silence before a format change and a DSD-silence ending on stop. M4c-2: diagnostics (done: a Diagnostics view with underruns, buffer and period, who handed the device over, the converter's clip count and peak, the kernel's `hw_params` and USB stream state including the DAC's feedback rate, what the driver accepts, and a copyable report). The design system, accessibility and keyboard shortcuts moved to the M5 design overhaul.
- [x] Take the DAC from PipeWire cleanly (M4c-2): before opening a device, request it through the device-reservation D-Bus protocol (`org.freedesktop.ReserveDevice1.Audio<card>`), which PipeWire/WirePlumber honor, and release it when Lyra closes the device or quits, so PipeWire takes it back. Today a DAC that is the OS output fails with "device busy". Built on the `dbus` crate (libdbus), already in the tree through Tauri, which also serves MPRIS later. Falls back to today's busy error when no session bus or no reservation is offered. Implemented in `audio-alsa/src/reserve.rs`: asked for only when the card is busy, held until the sink closes, release requests refused meanwhile. Known: WirePlumber 0.4 renames the card's nodes when it takes it back, so the desktop's default output may move.
- [x] Incremental scan and error states (M4a: size + mtime; unreadable files counted and reported; cancel keeps unreached files).

**Done when:** 10k-track scan remains responsive during playback.

## M5 — Production hardening

- [ ] Design overhaul before packaging, following [DESIGN_SPEC.md](DESIGN_SPEC.md) (from [DESIGN_Brief.md](DESIGN_Brief.md)), in six phases, one PR each:
  1. Foundations: tokens, vendored Manrope and icons.
  2. Shell and navigation.
  3. Player bar, accessibility and keyboard shortcuts.
  4. Signal path reported by UNDA, and Now Playing.
  5. Library screens.
  6. Settings.

  Each phase's outcome may adjust the later ones.
- [ ] Name the audio engine **UNDA** (Unified Native Digital Audio): rename the `player-core` crate to `unda` and use the name in the docs and UI (e.g. the about screen). Other crates keep their descriptive names. Before M6, so the Windows and macOS output crates start under it.
- [ ] Gapless and seek regression tests; unplug/replug tests; buffer underrun diagnostics.
- [ ] MPRIS integration and icons, on the D-Bus client added in M4c-2.
- [ ] Packages for recent distributions: `.deb`, `.rpm`, AppImage, Flatpak (needs explicit ALSA device access in the sandbox), AUR. Test each on a real install.
- [ ] Parser fuzzing, dependency license/security audit, log privacy review.
- [ ] Release notes, installation instructions, hardware compatibility report.

**Done when:** acceptance tests A01–A12 are documented with pass/fail evidence.

## M6 — Windows and macOS

Linux stays the reference platform. UI, library and engine are shared; each platform gets its own output crate behind `AudioSink`, with the same rule: the most direct path, exact format, no silent conversion.

- [ ] `audio-wasapi`: WASAPI exclusive mode for bit-perfect PCM and DoP; device enumeration, exact format negotiation, endpoint volume handling.
- [ ] Native DSD on Windows through ASIO where the DAC's driver provides it (check the Steinberg SDK licence first).
- [ ] `audio-coreaudio`: CoreAudio exclusive (hog mode) with the device's physical format set to the source format; DoP for DSD (CoreAudio has no native DSD).
- [ ] Hardware evidence per platform in `docs/hardware/`, as for Linux.
- [ ] Packaging and signing: Windows installer (MSI/NSIS) with code signing; macOS `.dmg` with notarization.

**Done when:** PCM and DoP are bit-perfect on both platforms with recorded hardware evidence, and installers are signed.

## Initial M0 prompt

> Read AGENTS.md, docs/SPECIFICATION.md, and docs/IMPLEMENTATION_PLAN.md. Implement milestone M0 only. Verify the Rust workspace and Svelte/Tauri app compile, install or document missing Linux prerequisites, add minimal IPC get_app_info, generate icons, and add CI. Do not implement audio playback yet. Run all required checks, report commands and results, and update milestone checkboxes only when validated.
