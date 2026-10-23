<p align="center">
  <img src="docs/assets/lyra-mark.svg" width="96" height="96" alt="">
</p>

<h1 align="center">Lyra</h1>

<p align="center">
  <b>Your music, exactly as it was recorded.</b><br>
  A Linux music player that sends every bit straight to your DAC. No mixer, no resampling, no surprises.
</p>

<p align="center">
  <a href="#why">Why</a> ·
  <a href="#what-reaches-your-dac">What works</a> ·
  <a href="#get-started">Get started</a> ·
  <a href="#build-it-yourself">Build</a> ·
  <a href="docs/hardware/">Hardware reports</a> ·
  <a href="docs/IMPLEMENTATION_PLAN.md">Roadmap</a>
</p>

> [!NOTE]
> **Pre-release.** Lyra plays, scans and organises a library today, and is being polished for its first packages. Build it from source for now.

## Why

On a Linux desktop, music usually goes through the sound server first: mixed with everything else, often resampled, sometimes turned down in software. You paid for a DAC; it rarely gets the file you bought.

Lyra takes the short road. Its engine, **UNDA**, written in Rust, opens the DAC directly through ALSA and hands it the samples untouched, at the file's own rate and depth. PCM stays PCM. DSD goes out as native DSD, or as DoP when that is what the DAC speaks. When something can't be played bit-perfectly, Lyra says so instead of quietly converting it.

And it stays light. No bundled browser, no background services. **Efficiency is a product goal**: every feature has to justify its memory and CPU.

- **Direct to hardware.** Rust straight to an ALSA `hw:` device. No Web Audio, PulseAudio or PipeWire in the playback path.
- **No hidden processing.** No resampling, no software volume. If the DAC can't take a format, you're told why.
- **Native DSD and DoP are separate.** Each is detected from the device at runtime, never assumed from a spec sheet.
- **Claims need evidence.** Bit-perfect is claimed after fixture tests and checks on real DACs.

## What reaches your DAC

| Source | Reaches the DAC as | Verified on hardware |
| --- | --- | --- |
| FLAC, WAV (16–32 bit, any rate the DAC takes) | PCM at the exact rate, lossless layout | iFi ZEN ONE: 16/44.1, 24/48, 24/96 · iFi USB Audio SE: 16/44.1, 24/96 |
| DSF, DFF (DSD64–DSD512) | Native DSD, otherwise DoP | ZEN ONE: native DSD64–256, DoP DSD64–128 · USB Audio SE: native DSD64 |
| DSF, DFF on a device without DSD (opt-in) | 24-bit PCM at 88.2 or 44.1 kHz, labelled *not bit-perfect* | Built-in audio (CS4208) |

Any ALSA device should work for PCM. DSD needs a DAC that exposes native DSD or decodes DoP. Per-DAC evidence lives in the [hardware reports](docs/hardware/).

## Features

- **Signal path, always visible.** Source, processing, DAC volume, output and device for the playing track, with a bit-perfect verdict.
- **A real library.** Add a folder (or drop it on the window). Albums, artists, tracks and cover art from FLAC, WAV, DSF and DFF; rescans only what changed.
- **Gapless queue and playlists**, keyboard shortcuts, media keys.
- **Tag tools that never touch your files.** Edit albums, tidy messy tags, match editions against MusicBrainz. Every change lives in Lyra and can be undone.
- **Hardware volume only.** The DAC's own control, from Lyra, the keyboard or your desktop's volume keys. Locked at 0 dB during DoP, where any change would turn DSD into noise.
- **DAC-friendly format changes.** A short silence while the DAC switches rate or mode, a warm-up for DACs that need one, and a clean DSD ending when you stop.
- **Survives unplugged drives.** A missing music folder keeps its albums and playlists until it's back.
- **Champagne or Arctic Blue**, always dark.

### Not there yet

- Packages (`.deb`, `.rpm`, AppImage, Flatpak, AUR)
- Read-ahead buffering for NAS and network shares
- Volume levelling (ReplayGain), through the DAC's own volume
- Windows and macOS

### Not for you?

Lyra plays **local files** on **Linux**, nothing else. If you stream, or want a server for every room, look at Roon or Audirvāna. If you want a light player on a headless box, MPD is excellent.

## Where your data goes

- **Nowhere, by default.** Lyra works offline. No account, no analytics, no telemetry.
- **Online lookups are opt-in.** Turned on, and only when you open ID Match on an album, Lyra sends that album's title and artist to MusicBrainz and asks the Cover Art Archive for covers.
- **Your music files are read-only to Lyra.** Edits, covers and playlists stay in Lyra's own library.

## Get started

**Linux.** Build from source (below). Lyra needs exclusive access to the DAC while it plays: if PipeWire is using it, Lyra asks for it politely and hands it back when playback stops. Desktop sound on that DAC is silent meanwhile.

**Windows and macOS.** Planned, after the Linux release ([M6](docs/IMPLEMENTATION_PLAN.md)).

## Other questions

**Why does it say "Not bit-perfect" when I turn the volume down?**
Lyra's stream is untouched, but most USB DACs apply their volume digitally, inside the DAC. At 0 dB nothing is changed anywhere.

**My DAC plays noise for a moment when the format changes.**
Raise *Resync delay* in Settings (0.5 s by default). Some DACs need a second to switch between rates or between PCM and DSD.

**Why Tauri and not a native toolkit?**
The interface uses the system's WebKitGTK, not a bundled browser, and never touches audio: UNDA and the library are plain Rust crates with no dependency on Tauri.

**Is AI used to build Lyra?**
Yes. Lyra is developed with AI coding assistants under written rules ([AGENTS.md](AGENTS.md)); every change goes through the same tests, lints and hardware checks.

## Build it yourself

Needs Rust (rustup), Node 24 and the Tauri Linux packages. Details in [Development](docs/DEVELOPMENT.md).

```sh
cd apps/desktop
npm ci
npm run tauri dev                         # development
CARGO_BUILD_JOBS=2 npm run tauri build    # release build and .deb
```

Run every check with `scripts/check.sh`.

```text
apps/desktop/          Svelte UI (no audio) and Tauri adapter (src-tauri/)
crates/unda            UNDA, the playback engine: controller and writer threads, native/DoP policy
crates/audio-alsa      ALSA discovery, exact-format output, DAC volume
crates/format-pcm      WAV/FLAC decoding to exact integer samples
crates/format-dsd      DSF/DFF parsing, DoP and native DSD packing
crates/dsd-pcm         Opt-in DSD→PCM conversion (filters, decimation, dither)
crates/library         SQLite library: scanner, tags, artwork, edits
crates/musicbrainz     Opt-in MusicBrainz and Cover Art Archive client
scripts/               check.sh, DAC probe, test fixtures, licence inventory
tests/e2e/             Real-window smoke tests (WebKitGTK + IPC)
```

| Read | For |
| --- | --- |
| [Specification](docs/SPECIFICATION.md) | What Lyra must do |
| [Implementation plan](docs/IMPLEMENTATION_PLAN.md) | Milestones and what's next |
| [Design spec](docs/DESIGN_SPEC.md) | The interface, tokens and components |
| [Development](docs/DEVELOPMENT.md) | Setup, checks, smoke tests |
| [Hardware reports](docs/hardware/) | Per-DAC evidence |
| [Agent rules](AGENTS.md) · [Third-party licences](docs/THIRD_PARTY.md) | Contributing, licences |

## License

[MIT](LICENSE). Font: Manrope (SIL OFL). Icons: Lucide (ISC). Metadata from MusicBrainz (CC0).

Lyra is not affiliated with iFi audio, Roon Labs or Audirvāna. Product names belong to their owners.
