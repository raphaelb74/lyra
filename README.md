# Lyra

A Linux-first, bit-perfect music player for PCM and DSD, written in Rust with a Tauri 2 + Svelte 5 interface.

> **Status: M4 in progress.** Plays FLAC and WAV bit-perfectly, and DSF/DFF as native DSD or DoP, to an ALSA `hw:` device. A music library scans your folders into albums with artwork; a gapless queue, playlists, Settings, opt-in DSD→PCM conversion and hardware volume are in. Diagnostics, keyboard shortcuts, design and packaging come next (see the [plan](docs/IMPLEMENTATION_PLAN.md)).

## What works

| Source | Reaches the DAC as | Verified on hardware |
| --- | --- | --- |
| FLAC, WAV (16–32 bit, any rate the DAC takes) | PCM at the exact rate, lossless layout | ZEN ONE: 16/44.1, 24/48, 24/96; USB Audio SE: 16/44.1, 24/96 |
| DSF, DFF (DSD64–DSD512) | native DSD (`DSD_U8`/`U16`/`U32`), else DoP | ZEN ONE: native DSD64–256, DoP DSD64–128; USB Audio SE: native DSD64 |
| DSF, DFF on a device without DSD (opt-in) | 24-bit PCM at 88.2 or 44.1 kHz, labelled not bit-perfect | built-in audio (CS4208) |

- **Library:** add a folder (button or drag and drop). Lyra reads tags and cover art from FLAC, WAV, DSF and DFF, rescans only what changed, and never writes to your music.
- **Bit-perfect status** for every track: format, rate, transport, and the DAC's own volume (Lyra sets a USB DAC's near-0 dB volume to exactly 0 dB while playing, then restores it).
- **Clear refusals** instead of conversions: a rate the DAC can't take, a busy device (and which program holds it), DSD on an output that can't decode it.

**DSD on built-in audio.** Built-in sound cards don't decode DSD, natively or as DoP. Sending them DoP would play loud noise, so by default Lyra refuses. In Settings you can turn on **Convert DSD to PCM**: DSD is then filtered and converted to 24-bit PCM at 88.2 kHz (44.1 kHz if that's all the device takes). It's only used when neither native DSD nor DoP can work, and it is never bit-perfect; the player always labels it "DSD converted to PCM".

**Volume.** Lyra never scales the samples: volume is the card's own hardware control. Use the control under Output (or Ctrl+↑/↓), or your desktop's volume keys and slider, which reach the same control through PipeWire; Lyra's slider follows them. On a USB DAC, anything below 0 dB usually changes the samples, and the bit-perfect status says so as soon as it happens. During DoP the volume must stay at 0 dB: Lyra locks its control, and stops playback at once if the volume moves anyway, before DoP turns into noise.

**Format changes.** Many DACs need a moment to switch between rates, or between PCM and DSD, and play noise if music arrives too early. Lyra plays a short silence first when the format changes (0.5 s by default, adjustable in Settings), and ends a DSD stream on DSD silence when you stop or quit.

**Hardware.** See the [hardware reports](docs/hardware/) for per-DAC evidence (iFi ZEN ONE, iFi USB Audio SE). Any ALSA device should work for PCM; DSD needs a DAC that exposes native DSD or decodes DoP.

## Principles

- **Direct to hardware.** Audio goes from Rust straight to an ALSA `hw:` device. No Web Audio, PulseAudio or PipeWire in the playback path.
- **No hidden processing.** No resampling or software volume. DSD→PCM conversion happens only if you turn it on, only as the last resort, and is always labelled. If the DAC can't take a format, Lyra says so.
- **Native DSD and DoP are separate.** Each is detected at runtime from ALSA, not assumed from a DAC's spec sheet.
- **Claims need evidence.** Bit-perfect output is claimed only after fixture tests and hardware verification.

## Quick start

Requires Rust (rustup), Node 24 and the Tauri Linux packages. See [Development](docs/DEVELOPMENT.md).

Lyra opens the DAC directly, so nothing else may use it while Lyra plays. If PipeWire is using the DAC, Lyra asks it to hand the DAC over and gives it back when playback stops; desktop sound on that DAC is silent meanwhile. Only a program that ignores this request (see [Development](docs/DEVELOPMENT.md#hardware-testing)) still makes Lyra report the device as busy.

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
crates/unda     playback engine: controller and writer threads, state, native/DoP policy
crates/audio-alsa      ALSA device discovery, exact-format output, DAC volume
crates/format-pcm      WAV/FLAC decoding to exact integer samples
crates/format-dsd      DSF/DFF parsing, DoP and native DSD packing
crates/dsd-pcm         opt-in DSD→PCM conversion (filters, decimation, dither)
crates/library         SQLite library: folder scanner, tags, artwork cache
scripts/               check.sh (all checks), DAC probe, test fixtures, license inventory
tests/e2e/             real-window smoke tests (WebKitGTK + IPC)
apps/desktop/tools/    vendored ESLint rules (anti-slop), run by `npm run lint`
.claude/               Claude Code agents and vendored Rust review skills
```

The audio engine and the library do not depend on Tauri. The UI talks to them only through Tauri commands, and plays only tracks inside your library folders.

## Documentation

- [Specification](docs/SPECIFICATION.md): what Lyra must do
- [Implementation plan](docs/IMPLEMENTATION_PLAN.md): milestones M0–M5
- [Development](docs/DEVELOPMENT.md): setup, checks, smoke tests
- [Hardware reports](docs/hardware/): per-DAC evidence · [M0 validation](docs/history/M0_VALIDATION.md)
- [Agent rules](AGENTS.md) · [Third-party licenses](docs/THIRD_PARTY.md)

## License

[MIT](LICENSE)
