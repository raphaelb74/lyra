# Lyra

A Linux-first, bit-perfect music player for PCM and DSD, written in Rust with a Tauri 2 + Svelte 5 interface.

> **Status: M4 in progress.** Plays FLAC and WAV bit-perfectly, and DSF/DFF as native DSD or DoP, to an ALSA `hw:` device. A music library scans your folders into albums with artwork. Queue, playlists, settings and packaging come next (see the [plan](docs/IMPLEMENTATION_PLAN.md)).

## What works

| Source | Reaches the DAC as | Verified on the iFi ZEN ONE |
| --- | --- | --- |
| FLAC, WAV (16–32 bit, any rate the DAC takes) | PCM at the exact rate, lossless layout | 16/44.1, 24/48, 24/96 |
| DSF, DFF (DSD64–DSD512) | native DSD (`DSD_U8`/`U16`/`U32`), else DoP | native DSD64, DSD128, DSD256; DoP DSD64, DSD128 |

- **Library:** add a folder (button or drag and drop). Lyra reads tags and cover art from FLAC, WAV, DSF and DFF, rescans only what changed, and never writes to your music.
- **Bit-perfect status** for every track: format, rate, transport, and the DAC's own volume (Lyra sets a USB DAC's near-0 dB volume to exactly 0 dB while playing, then restores it).
- **Clear refusals** instead of conversions: a rate the DAC can't take, a busy device (and which program holds it), DSD on an output that can't decode it.

**DSD on built-in audio.** Built-in sound cards don't decode DSD, natively or as DoP. Sending them DoP would play loud noise, so Lyra refuses. It doesn't convert DSD to PCM either; use a DAC with DSD support.

**Hardware.** See the [hardware reports](docs/hardware/) for per-DAC evidence. Any ALSA device should work for PCM; DSD needs a DAC that exposes native DSD or decodes DoP.

## Principles

- **Direct to hardware.** Audio goes from Rust straight to an ALSA `hw:` device. No Web Audio, PulseAudio or PipeWire in the playback path.
- **No hidden processing.** No resampling, software volume or DSD→PCM conversion. If the DAC can't take a format, Lyra says so.
- **Native DSD and DoP are separate.** Each is detected at runtime from ALSA, not assumed from a DAC's spec sheet.
- **Claims need evidence.** Bit-perfect output is claimed only after fixture tests and hardware verification.

## Quick start

Requires Rust (rustup), Node 24 and the Tauri Linux packages. See [Development](docs/DEVELOPMENT.md).

Lyra opens the DAC directly, so nothing else may use it while Lyra plays. If it reports the device is in use by PipeWire, switch the card's PipeWire profile off while listening (see [Development](docs/DEVELOPMENT.md#hardware-testing)).

```sh
cd apps/desktop
npm ci
npm run tauri dev                         # development
CARGO_BUILD_JOBS=2 npm run tauri build    # release build and .deb
```

Run every check with `scripts/check.sh`.

## Layout

```text
apps/desktop/          Svelte UI (no audio) and Tauri adapter (src-tauri/)
crates/player-core     playback engine: controller and writer threads, state, native/DoP policy
crates/audio-alsa      ALSA device discovery, exact-format output, DAC volume
crates/format-pcm      WAV/FLAC decoding to exact integer samples
crates/format-dsd      DSF/DFF parsing, DoP and native DSD packing
crates/library         SQLite library: folder scanner, tags, artwork cache
scripts/               check.sh (all checks), DAC probe, test fixtures, license inventory
tests/e2e/             real-window smoke tests (WebKitGTK + IPC)
```

The audio engine and the library do not depend on Tauri. The UI talks to them only through Tauri commands, and plays only files you dropped or tracks inside your library folders.

## Documentation

- [Specification](docs/SPECIFICATION.md): what Lyra must do
- [Implementation plan](docs/IMPLEMENTATION_PLAN.md): milestones M0–M5
- [Development](docs/DEVELOPMENT.md): setup, checks, smoke tests
- [Hardware reports](docs/hardware/): per-DAC evidence · [M0 validation](docs/M0_VALIDATION.md)
- [Agent rules](AGENTS.md) · [Third-party licenses](docs/THIRD_PARTY.md)

## License

[MIT](LICENSE)
