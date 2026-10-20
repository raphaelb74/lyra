# Lyra

A Linux-first, bit-perfect music player for PCM and DSD, written in Rust with a Tauri 2 + Svelte 5 interface.

> **Status: M1.** Plays FLAC and WAV bit-perfectly to an ALSA `hw:` device (verified on an iFi ZEN ONE, see [hardware reports](docs/hardware/)). Drop files onto the window to play them. DSD (M2–M3) and the music library (M4) are not implemented yet.

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
crates/player-core     playback engine: controller and writer threads, state, AudioSink
crates/audio-alsa      ALSA device discovery, exact-format output, DAC volume
crates/format-pcm      WAV/FLAC decoding to exact integer samples
crates/format-dsd      DSF/DFF parsing, DoP packing (M2)
scripts/               check.sh (all checks), DAC probe, test fixtures, license inventory
tests/e2e/             real-window smoke tests (WebKitGTK + IPC)
```

The audio engine does not depend on Tauri. The UI talks to it only through Tauri commands.

## Documentation

- [Specification](docs/SPECIFICATION.md): what Lyra must do
- [Implementation plan](docs/IMPLEMENTATION_PLAN.md): milestones M0–M5
- [Development](docs/DEVELOPMENT.md): setup, checks, smoke tests
- [Hardware reports](docs/hardware/): per-DAC evidence · [M0 validation](docs/M0_VALIDATION.md)
- [Agent rules](AGENTS.md) · [Third-party licenses](docs/THIRD_PARTY.md)

## License

[MIT](LICENSE)
