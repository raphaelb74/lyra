# Lyra

A Linux-first, bit-perfect music player for PCM and DSD, written in Rust with a Tauri 2 + Svelte 5 interface.

> **Status: M0 (application shell).** The window, mock library and Rust IPC work. Audio playback starts in M1. Nothing here plays audio yet.

## Principles

- **Direct to hardware.** Audio goes from Rust straight to an ALSA `hw:` device. No Web Audio, PulseAudio or PipeWire in the playback path.
- **No hidden processing.** No resampling, software volume or DSD→PCM conversion. If the DAC can't take a format, Lyra says so.
- **Native DSD and DoP are separate.** Each is detected at runtime from ALSA, not assumed from a DAC's spec sheet.
- **Claims need evidence.** Bit-perfect output is claimed only after fixture tests and hardware verification.

## Quick start

Requires Rust (rustup), Node 24 and the Tauri Linux packages. See [Development](docs/DEVELOPMENT.md).

```sh
cd apps/desktop
npm ci
npm run tauri dev
```

Run every check with `scripts/check.sh`.

## Layout

```text
apps/desktop/          Svelte UI (no audio) and Tauri adapter (src-tauri/)
crates/player-core     playback state machine, queue, transport policy
crates/audio-alsa      ALSA device discovery, negotiation, streaming
crates/format-pcm      PCM decoding
crates/format-dsd      DSF/DFF parsing, DoP packing
crates/library         SQLite library, scanner, metadata
scripts/               check.sh (all checks), license inventory
tests/e2e/             real-window smoke tests (WebKitGTK + IPC)
```

The audio engine does not depend on Tauri. The UI talks to it only through Tauri commands.

## Documentation

- [Specification](docs/SPECIFICATION.md): what Lyra must do
- [Implementation plan](docs/IMPLEMENTATION_PLAN.md): milestones M0–M5
- [Development](docs/DEVELOPMENT.md): setup, checks, smoke tests
- [M0 validation](docs/M0_VALIDATION.md): latest test evidence
- [Agent rules](AGENTS.md) · [Third-party licenses](docs/THIRD_PARTY.md)

## License

[MIT](LICENSE)
