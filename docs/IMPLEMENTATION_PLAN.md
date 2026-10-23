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
- [x] Album/artist/track browser, queue, playlist editing, Now Playing. (M4a: album grid and album tracks. M4b-1: engine queue with gapless continuation of same-format tracks, DoP marker phase kept across boundaries, next/previous/skip, Now Playing and Queue views. M4b-2: playlists (migration 2; entries keep their own id, follow their track out of the library; create, rename, delete, add, move, remove), Artists and Tracks views with search.)
- [x] Settings and diagnostics (M4c). M4c-1: Settings view (DSD transport, conversion, conversion gain, pause before playback on a format change) saved to `settings.json`; hardware volume control that follows the desktop's volume keys, with the bit-perfect verdict updated live and DoP stopped if the volume moves; silence before a format change and a DSD-silence ending on stop. M4c-2: diagnostics (done: a Diagnostics view with underruns, buffer and period, who handed the device over, the converter's clip count and peak, the kernel's `hw_params` and USB stream state including the DAC's feedback rate, what the driver accepts, and a copyable report). The design system, accessibility and keyboard shortcuts moved to the M5 design overhaul.
- [x] Take the DAC from PipeWire cleanly (M4c-2): before opening a device, request it through the device-reservation D-Bus protocol (`org.freedesktop.ReserveDevice1.Audio<card>`), which PipeWire/WirePlumber honor, and release it when Lyra closes the device or quits, so PipeWire takes it back. Today a DAC that is the OS output fails with "device busy". Built on the `dbus` crate (libdbus), already in the tree through Tauri, which also serves MPRIS later. Falls back to today's busy error when no session bus or no reservation is offered. Implemented in `audio-alsa/src/reserve.rs`: asked for only when the card is busy, held until the sink closes, release requests refused meanwhile. Known: WirePlumber 0.4 renames the card's nodes when it takes it back, so the desktop's default output may move.
- [x] Incremental scan and error states (M4a: size + mtime; unreadable files counted and reported; cancel keeps unreached files).

**Done when:** 10k-track scan remains responsive during playback.

## M5 — Production hardening

- [ ] Design overhaul before packaging, following [DESIGN_SPEC.md](DESIGN_SPEC.md) (from [DESIGN_Brief.md](DESIGN_Brief.md)), in six phases, one PR each:
  1. Foundations: tokens, vendored Manrope and icons. *Done.*
  2. Shell and navigation. *Done:* custom title bar; sidebar with Albums, Artists, Tracks, Playlists, Queue (with its length) and Settings at the bottom, an icon rail below 1000 px; Now Playing opens from the player bar, Diagnostics from Settings, both with Back and Escape; the engine connection shows only when it fails (version under Settings › About); Files view and dropped-file playback removed (Lyra plays from the library only); music folders in Settings.
  3. Player bar, accessibility and keyboard shortcuts. *Mostly done:* sliders, volume and device picker in the bar, short device names from Rust (USB DACs first), format line with the bit-perfect lamp; shuffle and repeat (off, all, one) beside the transport, kept by the engine across queues (not yet across restarts); the length under the seek bar switches to time left on click; the queue moved from the sidebar to a pane on the right (player-bar button, Ctrl+5) with Play next, Remove and Clear, and "Play next" / "Add to queue" in the library's right-click menus; Add to playlist matches Play album, announcements for screen readers; shortcuts (Space, Ctrl+←/→, Shift+←/→, Ctrl+., / and Ctrl+F, Ctrl+1–5, Ctrl+,, Escape; reload and print keys swallowed); focus rings for the keyboard only; Lyra's own right-click menus (Play, Open album, Go to album, Add to playlist…, Remove from playlist, Play from here) in place of the webview's. Up next reorders by dragging or Alt+↑ / Alt+↓ (engine `Edit::Move`). Add to playlist uses the shared popover. *Left:* lamp states checked on real hardware.
  4. Signal path reported by UNDA, and Now Playing. *Done, to check on hardware:* the engine reports its processing steps and the DAC's volume with the status; the signal path (source, processing, volume, output, device with the kernel's confirmation, verdict) opens from the player bar's format line and, collapsed, in Now Playing, which now shows artwork up to 560 px. *Left:* compare the stages with Diagnostics for PCM, native DSD, DoP and conversion on the SE.
  5. Library screens. *Started:* shared track rows and artwork; Artists as two panes like Apple Music (artist list with covers, the artist's albums and Play beside it). Album grid cards show title, artist and year from 512 px cover thumbnails (made on first view with gdk-pixbuf, cached in `artwork/thumbs`); the grid keeps its scroll position across an album visit; the album page shows year, genre (schema 3, placeholders like "Other" ignored), track count and length, format tags with Hi-Res, and Play, Shuffle and Add to playlist. *Left:* the 10k-track measurement.
  6. Settings. *Done:* compact rows with segmented controls; music folders; Output › Diagnostics; About with version and engine status. Licences in About (`scripts/record-notices.py` writes `src/assets/notices.json`; regenerate after lockfile updates). Keyboard shortcut list, from the same table as the key handling (`SHORTCUTS` in `shortcuts.ts`).

  Each phase's outcome may adjust the later ones.
  UI consistency pass (2026-10-23): [design revision](DESIGN_SPEC.md#colour) adopts Champagne/Arctic Blue accents, independent hover/selection/playing row states, shared primary/secondary/tertiary/icon controls and a local Appearance setting. Audio and hardware acceptance remain separate.
- [x] Name the audio engine **UNDA** (Unified Native Digital Audio): the `player-core` crate is now `crates/unda`; the name shows in Settings › About. Other crates keep their descriptive names.
- [x] Library preservation ([review](CURRENT_STATE_REVIEW.md) T01): a music folder that cannot be read, or that looks empty while the library has tracks in it (an unplugged drive), keeps its tracks and playlists and shows "not found" in Settings; tracks under an unreadable subfolder and known files that fail to read are kept; adding a parent folder moves the inner folders' tracks instead of deleting them. Removing a folder in Settings still removes its tracks.
- [x] DoP only to DACs that decode it ([review](CURRENT_STATE_REVIEW.md) T09): DoP goes to a device that also exposes native DSD, or that the user marked “plays DoP” under Settings › Output (`dopDevices` in `settings.json`); otherwise it is refused with that hint, or converted when conversion is on. The PCM carrier and an untouched volume are no longer taken as proof.
- [ ] Gapless and seek regression tests; unplug/replug tests; buffer underrun diagnostics.
- [ ] Read-ahead for network and slow storage. *Done, to check on hardware:* each open file has an I/O thread (`unda/src/readahead.rs`) holding about 30 s of the compressed file ahead of the decoder, capped at 4–48 MiB (DSD512 gets ~23 s), off the output thread. Seeks inside the window cost no I/O (a seek within the playing track keeps its file open); a seek further than 1 MiB past it jumps. Once a file is read to its end, the next track opens on a background thread, so a gapless boundary does not wait on the disk. The seek bar draws how far is read. *Left:* try on a real NAS with a stall (pull the cable mid-track). Known: Symphonia's FLAC reader cannot seek back once it has read to the end of a file, so seeks in the last moments of a track reopen it, as before.
- [ ] Hardware efficiency pass. *Done, to check during playback:* idle, packaged debug build under Xvfb, 20 s: 27.3 → 2.9 wakeups/s, CPU 0.9 → 0.1 %, 31 → 30 threads, RSS 179 → 188 MB (WebKit dominates; within run-to-run noise of a debug build). The volume watcher sleeps on the card's mixer events (`audio-alsa/src/events.rs`) instead of reopening the mixer every 100 ms, and reports to the engine only a settled change that differs from the open stream's; MPRIS sleeps on its bus connection; both wake through a socket pair (`src-tauri/src/wake.rs`) when the status changes. Paused playback no longer polls: free buffers return through the controller's inbox, and a paused writer waits to be woken (1 s safety net instead of 20 ms). Position polling only while playing and the window is visible. Left as is: the device list read twice a second (2 wakeups/s, a few hundred bytes; ALSA has no hot-plug event). No SIMD: nothing hot showed up. *Left:* measure during playback on the DAC (PCM, DoP, native DSD).
- [ ] Volume levelling (ReplayGain): read `REPLAYGAIN_*` / `R128_*` tags at scan; measure missing albums with EBU R128 (BS.1770) in a background, low-priority thread. Applied through the DAC's hardware volume, never by scaling samples, so the stream stays bit-perfect; album mode by default (gapless albums keep one level), track mode for shuffle. Off where the DAC has no hardware volume, and during DoP (locked at 0 dB). Shown in the signal path.
- [ ] MPRIS integration and icons, on the D-Bus client added in M4c-2. *Started:* `src-tauri/src/mpris.rs` serves `org.mpris.MediaPlayer2.lyra` (play/pause, next, previous, stop, seek, raise; status, title, artist, album, cover and length), so laptop media keys, the desktop's sound menu and `playerctl` control Lyra; the window also obeys media keys when it has focus. *Left:* test the media keys and sound menu while playing; volume through MPRIS; A12 evidence.
- [ ] Packages for recent distributions: `.deb`, `.rpm`, AppImage, Flatpak (needs explicit ALSA device access in the sandbox), AUR. Test each on a real install.
- [ ] Parser fuzzing, dependency license/security audit, log privacy review.
- [ ] Release notes, installation instructions, hardware compatibility report.

**Done when:** acceptance tests A01–A12 are documented with pass/fail evidence.

## M6 — Windows and macOS

Linux stays the reference platform. UI, library and engine are shared; each platform gets its own output crate behind `AudioSink`, with the same rule: the most direct path, exact format, no silent conversion.

- [ ] `audio-wasapi`: WASAPI exclusive mode for bit-perfect PCM and DoP; device enumeration, exact format negotiation, endpoint volume handling.
- [ ] Native DSD on Windows through ASIO where the DAC's driver provides it. Decided direction (2026-10-23), not done yet:
  - Licence: the ASIO SDK is dual-licensed (Steinberg proprietary or GPLv3, since October 2025). Before ASIO work starts, move the engine to GPL-3.0-or-later: `unda`, `audio-alsa` (and the coming `audio-wasapi`, `audio-asio`), `dsd-pcm`, `format-dsd`, `format-pcm`. The app, `library` and `musicbrainz` stay MIT; installers that link the engine ship as GPLv3 as a whole. Update each crate's `license` and `COPYING`, `deny.toml`, AGENTS.md's GPL rule and the README.
  - Interop: the SDK is C++ and the driver calls back on its own real-time thread through C function pointers. Prefer a small C++ shim exposing a C ABI, built with `cc`, behind one documented `ffi` module and a safe wrapper (as `cpal`'s `asio-sys`); `cxx` if the surface grows. Not zngur: it mainly exposes Rust to C++, and its CI does not cover MSVC.
  - Main work: ASIO pulls audio in a callback while UNDA's writer pushes blocking writes; bridge the buffer pool into preallocated callback buffers ([M6 plan](M6_WINDOWS_IMPLEMENTATION.md#6-optional-native-dsd-through-asio)).
  - Watch Microsoft's in-box USB Audio Class 2 driver with ASIO (preview announced for late 2026, Arm64 first): it would spare users vendor drivers; DSD support unknown. Lyra still builds against the SDK as a host.
- [ ] `audio-coreaudio`: CoreAudio exclusive (hog mode) with the device's physical format set to the source format; DoP for DSD (CoreAudio has no native DSD).
- [ ] Hardware evidence per platform in `docs/hardware/`, as for Linux.
- [ ] Packaging and signing: Windows installer (MSI/NSIS) with code signing; macOS `.dmg` with notarization.

**Done when:** PCM and DoP are bit-perfect on both platforms with recorded hardware evidence, and installers are signed.

## M7 — Wireless outputs (after v1)

Same rule as wired outputs: the most direct path, and the signal path says exactly what happens. UPnP can be bit-perfect; Bluetooth never is and is labelled lossy; AirPlay is parked.

- [ ] UPnP/DLNA output. Lyra acts as control point and media server; the network player (renderer) plays and keeps the clock, so this sits beside `AudioSink`, as a remote transport driven by UNDA's queue, not as a sink.
  - Discovery: SSDP search for `MediaRenderer` and OpenHome `Product` devices; device description and `ConnectionManager.GetProtocolInfo` give the formats and rates it accepts (FLAC, WAV/L16, DSF/DFF, DoP).
  - Serving: a small HTTP server on the LAN interface serves only queued tracks, under random per-session URLs, with Range for seeking; no directory listing, no paths in URLs; stops with the output.
  - Two modes, chosen per track from the renderer's formats: send the file untouched (the renderer decodes; bit-perfect up to the renderer) or, when the renderer lacks the format, send decoded PCM as WAV/L16 at the source rate. Never resample; refuse instead, as for ALSA.
  - Control: `AVTransport` (SetAVTransportURI, Play, Pause, Seek, Stop) with `SetNextAVTransportURI` for gapless; the OpenHome Playlist service where offered. `RenderingControl` volume when the renderer exposes it. Position from GENA events, polling as fallback.
  - Signal path: renderer name and model, what is sent (file or PCM), format reported by the renderer; verdict "bit-perfect to the renderer" only when the bytes leave Lyra untouched.
  - Dependencies: reuse `quick-xml` and `ureq`, already in the tree; small std UDP code for SSDP. Pick the HTTP server (std `TcpListener` or `hyper`, already in the tree) at design time.
- [ ] Bluetooth: the best available lossy path, never called bit-perfect. Output through PipeWire's BlueZ sink at the source rate when the codec allows it (LDAC 44.1–96 kHz; aptX HD 48 kHz). Show the negotiated codec and bitrate (BlueZ `MediaTransport1`) in the signal path, with a "lossy wireless" verdict. Documented alternative today: a USB Bluetooth transmitter (LDAC / aptX Adaptive) is a normal USB audio device, so the existing ALSA path reaches it bit-perfect.
- [ ] AirPlay (parked): ALAC at 16/44.1 (some AirPlay 2 receivers take 24/48), so it is bit-perfect only for CD-rate sources and needs downsampling otherwise. Sending does not need Apple's SDK (open implementations such as OwnTone exist), but AirPlay 2 pairing and timing make it a large subsystem to maintain. Reconsider only on user demand.

**Done when:** a FLAC, a hi-res PCM file and a DSF play gaplessly to at least two UPnP renderers from different vendors, with the bytes received checked against the source where the renderer allows it; Bluetooth shows its codec and lossy verdict.

## Initial M0 prompt

> Read AGENTS.md, docs/SPECIFICATION.md, and docs/IMPLEMENTATION_PLAN.md. Implement milestone M0 only. Verify the Rust workspace and Svelte/Tauri app compile, install or document missing Linux prerequisites, add minimal IPC get_app_info, generate icons, and add CI. Do not implement audio playback yet. Run all required checks, report commands and results, and update milestone checkboxes only when validated.
