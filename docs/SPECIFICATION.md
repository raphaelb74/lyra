# Product and technical specification

**Project:** Lyra (identifiers and package names: `lyra`)  
**Version:** Draft 0.1  
**Platform:** Linux x86_64 first (Linux Mint 22.3 / Ubuntu-based)  
**Primary validation DAC:** iFi ZEN ONE (USB `20b1:3009`, release 1.0c), see [hardware report](hardware/ifi-zen-one.md). Lyra must work with any ALSA DAC; nothing is specific to one model.  
**Target stack:** Rust + ALSA + Tauri 2 + Svelte 5 + TypeScript

## 1. Product objective

Build a local-first, lightweight, attractive graphical music player with deterministic audio output. Support bit-perfect PCM and DSD to a compatible USB DAC, without implicit DSP, resampling, software gain, or browser audio processing.

**Non-goals for v1:** streaming services, UPnP, multi-room, DSP/upsampling, remote access, Windows/macOS, network library, automatic DSD-to-PCM conversion, SACD ISO extraction, MQA decoding, audio recording.

## 2. Core user stories

1. As a listener, I can select a music folder, scan it, and browse albums with artwork and metadata.
2. As a listener, I can play FLAC/WAV with the exact sample format and rate accepted by my DAC.
3. As a listener, I can play DSF/DFF as native DSD when Linux/ALSA exposes it, or DoP when supported.
4. As a listener, I can see *source format*, *output transport*, *effective output rate*, and *device name* separately.
5. As a listener, I can choose a specific ALSA hardware endpoint and get an actionable error if it is busy/unplugged.
6. As a listener, I can seek, pause, skip, and play gaplessly when supported by the selected formats and hardware.
7. As a listener, I can control playback using Linux MPRIS media keys.

## 3. Architecture and ownership

```text
apps/desktop/src       Svelte UI; no audio decoding/output
apps/desktop/src-tauri Tauri IPC adapter and lifecycle
crates/player-core     Playback state machine, queue, transport policy
crates/audio-alsa      Device enumeration, negotiation, ALSA streaming
crates/format-dsd      DSF/DFF parsers and DoP packer
crates/format-pcm      PCM decode interface (Symphonia integration)
crates/library         SQLite schema, scanner, metadata/artwork
```

The engine should expose control messages (play/pause/stop/seek/select device) over channels and publish immutable state snapshots/events. Use plain threads and channels; the engine does not need an async runtime, and the Tauri adapter bridges to Tauri's own async layer. Use an audio writer thread isolated from the UI and database. A bounded decoded-frame ring buffer feeds the writer; define backpressure, cancellation, underrun, and shutdown semantics. Keep format parsing and transport encoding testable without a DAC.

### Playback state machine

`Stopped → Loading → Negotiating → Buffering → Playing ↔ Paused → Stopped`; transitions to `Error` on fatal failure and `Recovering` for unplug/replug if explicitly enabled. All state transitions are observable by the UI. Stopping drains/discards appropriately and closes ALSA resources. Do not automatically switch DACs.

## 4. Audio transport requirements

### PCM

- Open exact ALSA hardware endpoint (`hw:CARD=...,DEV=...` or stable equivalent), not `default`, `plughw`, PulseAudio or PipeWire.
- Negotiate a supported exact sample rate and representation; reject unsupported settings rather than resample silently.
- Avoid software gain; optionally expose *hardware* volume only if hardware actually supports it, with bit-perfect warning.
- Ensure channel mapping and sample-endianness are explicit.

### DSD

- Parse DSF and DFF/DSDIFF, validate headers, channel count, rate, data bounds, and byte/bit ordering; preserve raw DSD bits.
- For **native DSD**, enumerate ALSA hardware formats (such as `DSD_U8`, `DSD_U16_LE`, `DSD_U32_LE` where supported), check driver capabilities, and convert packing/order only as required by the selected ALSA representation; never assume any particular format is exposed.
- For **DoP 1.0**, package 16 DSD bits per channel into a 24-bit PCM frame, with marker bytes alternating `0x05` / `0xFA` per sample frame and matching markers across channels. Use a supported 24-in-32 or packed-24 ALSA format and verify byte ordering for the negotiated format.
- DoP carrier sample rate = DSD bit rate / 16: DSD64 → 176.4 kHz; DSD128 → 352.8 kHz; DSD256 → 705.6 kHz. A DAC with a 384 kHz PCM interface cannot receive DSD256 via standard DoP; native DSD would be required.
- Detect native DSD and DoP independently; make transport choice explicit: `Auto (native then DoP)`, `Native only`, `DoP only`. If unsupported, report why. No automatic DSD→PCM fallback in v1.
- Disable crossfade, replay gain, EQ, and software volume in bit-perfect mode.
- Verify initial and final partial DSD blocks, silence pattern, and marker phase across gapless track boundaries. Do not insert PCM silence as a DSD substitute.

### Audio integrity

- Compare decoded/transported payload against deterministic fixture hashes; validate DSF block deinterleaving and DFF sample order.
- Record ALSA negotiated format/rate, buffer and period sizes, underruns, device IDs, and transport mode.
- Use a compatible DAC's DSD indicator as supporting evidence, not sole proof of bit-perfect output.

## 5. Device capability discovery

Enumerate ALSA cards, devices, hardware PCM rates/formats, and whether exclusive open succeeds. Persist stable identifiers where possible; card indices may change after reconnect. Surface firmware/kernel/ALSA details in diagnostics. If the DAC is busy, explain which device failed to open; never route to the default sound card silently.

**Hardware gate (every DAC):** obtain `aplay -l`, `/proc/asound/card*/stream0`, ALSA format probe, USB VID/PID, model revision, and firmware version (`scripts/probe-dac.sh` collects these), and record them in `docs/hardware/<model>.md`. Do not promise native DSD256 before confirming an exposed and functioning native DSD ALSA path. Descriptor evidence for the ZEN ONE is recorded; playback evidence is pending.

## 6. Music library

- Local folder import; recursive scanning off UI thread; incremental updates; metadata tags and embedded/cover.jpg artwork.
- SQLite tables: `tracks`, `albums`, `artists`, `folders`, `playlists`, `playlist_items`, `scan_runs` with schema migrations.
- Index by normalized paths and filesystem identity where feasible; handle moved/unavailable files without data loss.
- Initial formats: DSF, DFF, FLAC, WAV; optionally AIFF/MP3 later.
- Metadata scanning must not block playback. Album-art cache should be bounded.

## 7. User interface

- Tauri 2 shell, Svelte 5, TypeScript strict, accessible components, keyboard controls, responsive dark-first design.
- Views: Library (Albums/Artists/Tracks), Now Playing, Queue, Playlists, Settings/Audio Devices, Diagnostics.
- Now Playing: large artwork, progress/seek, transport controls, source badges (`DSD128`, `FLAC 24/96`), output badges (`Native DSD`, `DoP`, `PCM`), DAC name, explicit bit-perfect status.
- Settings: output device, native/DoP policy, audio buffer, scan folders; destructive actions require confirmation.
- Errors: unsupported format/rate, exclusive device busy, unplug, invalid file, buffer underrun; actionable wording.
- System integration: MPRIS, desktop media keys, notifications where enabled, Linux desktop file and icons.

## 8. Security and reliability

- Tauri allowlist: minimum required commands, no generic shell execution, no remote content by default, strict CSP.
- Scan local files only from user-selected directories. Treat tags/artwork as untrusted; cap allocation sizes and guard integer overflow in binary parsers.
- Persist app configuration atomically; graceful shutdown and recovery; structured logs without user media paths by default.
- No telemetry or remote services in v1.

## 9. Quality attributes

- Responsive UI during scan and playback; no audio writer thread waits on UI/database.
- Gapless transitions for compatible consecutive tracks; no clicks from incorrect DSD marker handling.
- Cold start and library scan benchmarks documented on target hardware; avoid unsupported performance promises.
- Unit, integration, parser fuzzing, and manual hardware tests. Run CI without a DAC via a mock sink.

## 10. Acceptance tests

| ID | Test | Expected result |
|---|---|---|
| A01 | Open 16/44.1 and 24/96 FLAC/WAV | Exact rate and payload representation; no resampling |
| A02 | DSF DSD64 / DSD128 DoP | Alternating marker pattern, correct interleave, compatible DAC indicates DSD |
| A03 | DFF DSD64 | Parser preserves bit order and channel ordering |
| A04 | DSD256 on iFi | Native only if ALSA exposes and hardware accepts it; otherwise explicit unsupported message |
| A05 | Device disconnected mid-track | Controlled stop/error, no crash or silent device switch |
| A06 | Another process holds DAC | Clear busy error, no fallback to mixer |
| A07 | Scan 10k tracks during playback | No blocking UI; no scan-caused playback stalls in target benchmark |
| A08 | Consecutive same-format tracks | Gapless without spurious silence/clicks |
| A09 | Seek DSF and FLAC | Correct frame position; buffers reset and output resumes cleanly |
| A10 | UI requests unsupported output mode | Visible explanation; no silent conversion |
| A11 | Tauri permissions audit | No unnecessary filesystem or shell permissions |
| A12 | MPRIS media key controls | Correct state and metadata exposed |

## 11. Decisions pending hardware testing

- ZEN ONE exposes `DSD_U32_BE` (native DSD up to DSD256 by descriptor). Does playback confirm it, and which USB quirks apply? Is its USB volume control digital (bit-perfect only at maximum)?
- How to get exclusive access while PipeWire is running? When WirePlumber has the USB DAC as an active sink it usually holds the `hw:` device, so a direct open fails with `EBUSY`. Expect this as the common case on Mint, not an edge case: detect it, report it clearly (A06), and document how to release the device (e.g. disable the DAC's PipeWire node or use device reservation).
- What period/buffer sizes provide stable playback without excessive latency?
- Which exact metadata parser supports required DSF/DFF tags? Verify crate capabilities before adoption.
- How to manage PCM↔DSD and rate changes without audible transients?
