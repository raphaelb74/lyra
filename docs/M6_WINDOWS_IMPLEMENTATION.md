# M6 Windows: analysis, specification and implementation plan

Date: 22 October 2026. Status: proposed, not implemented or hardware-verified. This expands the Windows portion of [IMPLEMENTATION_PLAN.md, M6](IMPLEMENTATION_PLAN.md#m6--windows-and-macos). macOS/CoreAudio remains a separate part of M6 and is not completed by this plan. Linux remains the reference platform; its current v1 ALSA-only policy stays in force until Windows implementation is explicitly scheduled.

Related: [current-state review](CURRENT_STATE_REVIEW.md), [metadata specification](METADATA_SPECIFICATION.md) and [metadata plan](METADATA_IMPLEMENTATION_PLAN.md).

## 1. Feasibility and existing groundwork

Windows support is feasible. Tauri/Svelte, the pure Rust decoders, DSD packing, conversion and most UNDA playback control can be shared. The repository already has the correct high-level M6 direction: WASAPI exclusive PCM/DoP and optional ASIO native DSD. There is no `audio-wasapi` or `audio-asio` implementation in the reviewed tree.

The main work is broader than a sink: desktop code directly exposes ALSA types, library paths and timestamps use Unix APIs, artwork uses gdk-pixbuf, desktop controls use D-Bus, and packaging/CI target Linux. Resolve these boundaries while preserving Linux behavior and fixtures.

Recommended first Windows release: **Windows 11 x86_64, native MSVC build, WASAPI exclusive PCM, and DoP only on verified compatible endpoints**. This is a proposed product target, not a claim about Tauri's complete minimum OS support. Windows 10, ARM64 and 32-bit builds are outside the initial acceptance matrix. ASIO native DSD is separately gated and must not block the initial Windows PCM/DoP release.

## 2. User-facing specification

1. Install a signed Windows package, import local music and use the same library/playlist/editor UI as Linux.
2. Select a named, persistent output endpoint and see **WASAPI exclusive** separately from **PCM**, **DoP** or **DSD converted to PCM**.
3. Play supported integer PCM at the source rate/channel count with lossless sample representation; refuse unsupported formats instead of resampling, converting to float or selecting shared mode silently.
4. Play DSD64/128 through DoP only if the endpoint, driver, transport packing and DAC support it. DoP is a transport, not PCM conversion.
5. Retain current explicit DSD policies: Native-only refuses without native support; DoP-only refuses without DoP; Auto on the selected WASAPI endpoint tries supported DoP and then optional labelled PCM conversion only if enabled. No automatic backend/device switch to ASIO or the default speakers.
6. Report exclusive access disabled, endpoint busy/unplugged, audio service unavailable, unsupported format and unverified DoP with actionable messages.
7. Keep playback on the chosen device through default-device changes; stop with a visible error when it disappears. Reconnection makes it selectable again without auto-resume or switching outputs.
8. Use media keys/system transport controls without passing audio through a Windows media player object.
9. Display actual output format, driver/endpoint identity, buffer geometry, glitches and the limits of verification. No Linux-style kernel/USB feedback fields are fabricated on Windows.

WASAPI exclusive may silence other applications on the endpoint. Explain that when the user selects it and release on stop/exit. Do not silently add a shared-mode fallback. Microsoft's general guidance discusses shared-mode interoperability; Lyra deliberately selects exclusive mode for its exact-output requirement. [Microsoft exclusive-mode documentation](https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams).

## 3. Concrete portability inventory

| Current location                                | Assumption/blocker                                                             | Required integration                                                                                                                       |
| ----------------------------------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Root workspace / `crates/audio-alsa/Cargo.toml` | ALSA crate is a workspace member; desktop dependency is unconditional          | Target-gate ALSA dependencies/source/tests/examples so native Windows workspace checks do not build `alsa-sys`; add Windows backend member |
| `src-tauri/Cargo.toml`                          | Unconditional `audio-alsa`, `dbus`, `gdk-pixbuf`                               | Linux target dependency tables; Windows bindings/backend dependencies; portable image implementation                                       |
| `src-tauri/src/lib.rs`                          | Constructs `AlsaSink`, gets ALSA stats, starts volume/MPRIS workers            | Platform factory/facade; initialize Windows objects on their owner threads                                                                 |
| `src-tauri/src/commands.rs`                     | Discovery, volume, probe and diagnostic commands call ALSA directly            | Shared device/report contracts implemented by platform adapters                                                                            |
| `src-tauri/src/ipc.rs`                          | ALSA types/format labels and procfs reports in common IPC                      | Stable common fields plus tagged backend-specific details; update TS guards/tests/UI                                                       |
| `src-tauri/src/volume_watch.rs`                 | Reads ALSA mixer state periodically                                            | Windows endpoint callbacks/control classification and stream-impact reporting                                                              |
| `src-tauri/src/mpris.rs`                        | Linux session D-Bus                                                            | Keep Linux module; Windows SMTC integration via window interop                                                                             |
| `src-tauri/src/thumbs.rs`                       | gdk-pixbuf decoder                                                             | Portable bounded image decode/resize; preserve supported image formats and cache behavior                                                  |
| `crates/library/src/db.rs`                      | Unix raw-byte BLOB paths with `OsStrExt`                                       | Versioned platform-native path codec and path identity policy; preserve Linux bytes losslessly                                             |
| `crates/library/src/scan.rs`                    | Unix `mtime`/`mtime_nsec`; path-based identity                                 | Portable timestamps/file IDs; Windows case/volume/reparse behavior and complete traversal results                                          |
| `crates/unda/src/sink.rs`                       | `AudioSink: Send`; methods may block; native candidates use ALSA-style layouts | Thread-owned backend factory, cancellation/clock contract, selected-backend candidate filtering                                            |
| `src-tauri/src/ipc.rs`, artwork CSP             | Hardcoded `lyra-art://localhost/`                                              | Platform-aware custom-protocol URLs and matching CSP for WebView2                                                                          |
| Tauri config/icons                              | Only `deb` configured; `.ico` ignored by Git                                   | Windows config overlay, generated Windows icon and NSIS/MSI target decision                                                                |
| CI / `scripts/check.sh` / smoke tests           | Linux packages, Bash, Xvfb, WebKitGTK and `.desktop` checks                    | Native Windows CI/check entry point, WebView2 smoke tests; retain Linux job                                                                |
| `deny.toml`, notices                            | Dependency policy target is Linux only                                         | Add Windows target audit and regenerate inventory/notices for shipped dependencies                                                         |

These are reviewed code paths, not a successful cross-build. Adding only `cfg` around `AlsaSink::new` would leave several compile and runtime blockers unresolved.

## 4. Shared architecture and threading

Keep `AudioSink` in UNDA. Add a small platform-neutral output facade covering endpoint descriptors/capabilities, volume state, output statistics and backend diagnostics. Platform-specific raw information belongs in tagged structures, not a common report named `alsa`. Prefer modules or a small cohesive crate to a broad plugin framework.

An endpoint identity is opaque and namespaced, for example `(backend, endpoint_id)`. Persist the Windows endpoint ID, not a display name, ordinal or default-device role. Display friendly name, interface and backend separately. Device IDs may change after driver reinstall; surface a missing preference and require explicit reselection. The same physical DAC exposed through WASAPI and ASIO represents two explicit selectable endpoints.

Shared flow:

```text
Svelte UI → typed Tauri commands → UNDA controller/decoder
                                      ↓ reusable bounded buffers
                              UNDA writer, owns backend
                                  ├─ Linux: ALSA hw
                                  └─ Windows: WASAPI exclusive

device/volume/media-control workers → snapshots/events → UI/controller
```

### COM ownership and `Send`

The existing app constructs a sink in setup and moves it into the writer. Windows COM interfaces cannot be assumed safe to create on one apartment and move to another. Construct an inert `Send` sink configuration or add a writer-side factory; initialize COM and create audio interfaces on the thread that owns/uses/releases them. Pair initialization/cleanup on that thread. Enumeration/media-control threads have their own documented apartment and callback lifetime rules. Do not add an unsafe `Send` implementation merely to satisfy the trait.

Use RAII wrappers around handles, COM resources, event registration and MMCSS registration. Keep unavoidable Windows FFI `unsafe` narrow with soundness notes and tests. Device callbacks enqueue bounded/coalesced notifications; they do not call back into the UI, database or blocking playback control while holding driver locks.

### Existing sink-contract issues to resolve first

`write`/`drain` currently receive no cancellation token/deadline, and engine position counts submitted frames. Before Windows implementation, define bounded cancellation for stop/seek/shutdown/unplug and a backend-rendered position/latency snapshot. A small shared cancellation primitive visible to the sink avoids waiting indefinitely behind a blocked write; do not turn `close` into a UI-thread device call. Mock cancellation tests must pass on Linux too.

The existing eight-buffer pool bounds payloads but channels need a measured bounded/allocation-free implementation for the writer guarantee. Backend packet staging must be preallocated at open time. Control/device/metadata notifications use bounded queues or coalesced snapshots with documented overflow behavior; stop/error transitions cannot be dropped silently.

## 5. WASAPI backend design

Create `crates/audio-wasapi`, preferably using the Microsoft-maintained Rust `windows` bindings with only required API features. Select the exact version and record its licence during implementation; no dependency is added now. Use direct WASAPI rather than a general audio wrapper unless that wrapper can prove the same exclusive/exact-format/control guarantees.

### Discovery and exact negotiation

Enumerate active render endpoints with the MMDevice API. Collect opaque endpoint ID, friendly name, relevant device/driver identity where available, and runtime state. Probe exact candidate formats for the selected source, not an assumed exhaustive list of all rates. Never label an unprobed format supported.

Use `IAudioClient::IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, …)` with exact integer PCM format descriptors. Accept only exact support. Do not use `GetMixFormat` as the desired output or apply shared-mode closest-match formats. Represent container bits, valid bits, block alignment, channel mask/order and sample rate explicitly; test 16-bit, packed 24-bit and 24/32-bit containers with existing lossless packing fixtures. Reject float-only or incompatible layouts in this initial integer-only release. [Exact format probing](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-isformatsupported).

Initialize an event-driven exclusive client, handle period/buffer alignment and reactivation as required, register its event before starting, obtain the actual buffer size and preallocate staging. Record requested versus initialized format and driver result; this is host API evidence, not an independent measurement inside the DAC. [Client initialization and alignment rules](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize).

### Streaming, packet boundaries and cancellation

Translate UNDA chunks into device packets; decoder blocks need not match the WASAPI buffer. For event-driven exclusive rendering, obey the full-buffer contract rather than assuming arbitrary partial releases. Carry unfinished packets across chunks and compatible track boundaries with preallocated storage. Do not pad every source block: that would insert gaps/corrupt DoP.

On a true final partial packet, append declared transport-appropriate tail silence only after the last valid frame, without including tail frames in the musical position. PCM uses zero samples; DoP uses DSD idle payload with the correct continuation marker phase. Never use an OS generic silent-buffer flag to fill a DoP packet. Define pause, discard/seek, drain and shutdown to clear staged old-generation bytes without losing marker phase within a continuing stream.

Wait for render readiness with a stop/cancellation event and finite watchdog/deadline. Never spin or wait forever. MMCSS may prioritize the writer, but no DB/network/log formatting/file I/O occurs there. Error events carry compact structured data; user-facing formatting happens outside steady-state writing.

Track frames submitted and rendered through audio clock/padding information supported by the chosen stream, including latency and backend tail accounting. Same-format tracks retain the client for gapless playback; format/rate changes reopen with the configured settling delay. Reuse generic resync and DSD-tail policy, but do not copy Linux's device warm-up workaround to Windows without Windows evidence.

### DoP capability and safety

WASAPI receives DoP as an exact high-rate integer PCM carrier. Existing packers/fixture marker tests are shared. DSD64 requires 176.4 kHz, DSD128 352.8 kHz, DSD256 705.6 kHz. A 384 kHz endpoint therefore cannot carry DSD256 via standard DoP; it may still support native DSD through a different vendor API.

PCM format acceptance does **not** establish that a DAC decodes DoP. Neither a USB label nor a unity volume alone establishes it. Separate `carrier_supported`, `dop_confirmed` and `volume_integrity` states. Initial eligibility comes from a documented Windows DAC/driver verification record or an explicit user configuration identifying a known compatible DoP DAC; unknown speakers remain refused. Do not run audible automatic DoP test probes. Report an unverified hardware claim conservatively.

Do not submit DoP before volume/mute behavior and bit-preserving carrier layout are established. Keep markers aligned across channels/tracks and use transport-aware underrun/tail behavior. For underrun, favor a controlled stop/error for DoP/native streams unless a tested recovery can preserve transport phase; PCM recovery is counted and reported. Do not fabricate success when the driver underruns silently.

### Windows volume is not the ALSA mixer

Session volume interfaces do not control exclusive streams. Endpoint volume may be implemented in hardware or software; software endpoint controls are bypassed by exclusive mode, whereas hardware controls can affect it. Query hardware support, effective dB/mute state and callbacks, and distinguish a bypassed OS slider from controls that alter the actual stream. Never implement a missing exclusive volume slider by multiplying samples. [Microsoft endpoint-volume behavior](https://learn.microsoft.com/en-us/windows/win32/coreaudio/endpoint-volume-controls).

Disable/hide ineffective controls with an explanation. Lock Lyra's gain/mute controls during DoP and stop on an external hardware control change that threatens integrity; report callback timing limits instead of guaranteeing zero invalid frames. Unknown hardware processing yields an unknown integrity verdict. Do not reuse Linux's `None`/`Unity` interpretation without an explicit Windows capability classification. Avoid automatically raising master volume: any unity-setting behavior must be an explicit, documented device policy with safe restoration.

### Errors and device lifecycle

Map Windows failures into existing broad sink errors plus diagnostic details: unsupported exact format, exclusive access denied/disabled, device busy, endpoint invalidated, service unavailable and other driver failure. Do not claim to know the holding process if Windows does not expose it. Preserve HRESULTs outside user-friendly wording for reports.

Use `IMMNotificationClient` for endpoint changes and callbacks for relevant volume changes. Stop safely on unplug, sleep/resume invalidation or driver reset; explicitly reopen only after a user action initially. Default-role changes do not migrate an active stream. Release exclusive resources promptly on stop/exit and test all cleanup paths.

## 6. Optional native DSD through ASIO

WASAPI in this design provides PCM/DoP, not a standard portable native-DSD path. Native DSD needs an ASIO driver that explicitly supports DSD I/O, its required bit order/sample type and supported rates. A vendor saying “supports DSD” is insufficient evidence for the Windows API path.

Treat `audio-asio` as a separate backend, selected by the user. Probe installed driver/runtime capabilities; keep vendor driver installation separate and never bundle a driver without its redistribution rights. Document DAC model/firmware, exact driver version, architecture, ASIO DSD mode and measurements. Never automatically choose a similarly named driver or pretend ASIO4ALL establishes native DSD.

Steinberg currently presents an open-source GNU ASIO variant and a proprietary licensing route, unlike its MIT VST technology. Lyra's no-unapproved-GPL policy makes SDK/binding/redistribution review a release gate. A permissive Rust wrapper does not settle the underlying SDK terms. Document an approved route before code adoption or distribution; no automatic GPL dependency is proposed. [Steinberg developer portal](https://www.steinberg.net/developers/).

ASIO uses driver callbacks and planar/double buffers, so it cannot simply reuse a blocking WASAPI write implementation. A future design must bridge UNDA's bounded buffer pool into preallocated callback buffers, pack DSD appropriately, handle reset/rate-change messages, and define safe underrun behavior without allocation, locks or logging inside callbacks. The current ALSA native candidate layouts must be filtered/replaced by the selected ASIO driver's actual types. Sharing DSD test vectors is valuable; sharing ALSA format names as the Windows API is not.

First Windows release wording should explicitly say **WASAPI PCM/DoP; native ASIO DSD pending** until its separate gate passes. Native-only on WASAPI returns a clear error even if another ASIO endpoint exists.

## 7. Library, artwork and desktop integration

### Paths and persistence

Do not convert Linux raw paths or Windows wide paths to lossy UTF-8 for identity. Add a versioned platform-native path representation (Linux bytes; Windows lossless UTF-16 units), including platform/root identity. Keep a display string separate. Preserve existing Linux BLOB paths through a migration; a Linux database copied to Windows cannot directly play `/home/...` paths and needs a deliberate root-relocation/import workflow.

Test case behavior per filesystem/directory, drive letters and volume identity, removable volumes, non-BMP names, non-UTF-8 Linux paths, long paths, junctions/reparse points and link cycles. Do not lowercase all paths or treat a textual `starts_with` as a security boundary. Skip unsupported reparse traversal with a visible status; missing/incomplete roots retain library entries and playlist IDs.

Use portable `modified()`/`SystemTime` observations plus platform file identity where needed; document timestamp granularity and race handling. Store size/mtime as scan hints, not proof of identity for future tag writes. Tauri's app data/config/cache directories provide native locations. Test settings replacement, concurrent temp naming, durability and corrupted settings recovery on NTFS; do not assume the current Linux write/rename pattern establishes Windows crash safety.

If metadata file writing ships, coordinate source handles and atomic replacement on Windows before enabling it. Read-only or sharing violations are actionable errors, never a reason to truncate files. Backups and filesystem tests must cover ACLs, disk-full and external editor races.

### Artwork and WebView2

Replace gdk-pixbuf in shared thumbnail code with a portable bounded decoder/resizer or a platform image adapter. Preserve limits and original-versus-thumbnail MIME correctness. Tauri custom protocols use an HTTP(S) localhost mapping on Windows rather than the Linux literal scheme; generate artwork URLs with that platform behavior and update narrowly scoped `img-src` origins. Do not add arbitrary HTTP image/network access. [Tauri custom-protocol configuration](https://v2.tauri.app/reference/config/#windowconfig).

### Media controls and window behavior

Replace MPRIS with manually managed Windows System Media Transport Controls via `ISystemMediaTransportControlsInterop::GetForWindow` using the native Tauri HWND. Publish metadata/timeline/playback state and route events to UNDA. This is desktop-control integration only; keep audio in the Rust output path. Verify focused keys and system callbacks do not double-trigger actions. [Microsoft desktop SMTC interop](https://learn.microsoft.com/en-us/windows/win32/api/systemmediatransportcontrolsinterop/nf-systemmediatransportcontrolsinterop-isystemmediatransportcontrolsinterop-getforwindow).

Validate custom titlebar drag regions, maximize/restore, DPI changes, multi-monitor placement, keyboard menus, focus, reduced motion and accessibility using WebView2. Handle Unicode paths in folder picking/drag-drop and Windows newline/clipboard behavior. Platform-irrelevant Linux controls/reports are absent or explicitly unavailable, rather than empty ALSA errors.

## 8. Packaging, CI and validation

Build natively on Windows with the pinned stable Rust/MSVC toolchain, Windows SDK/Build Tools, Node version matching the project and WebView2 prerequisites. Retain Linux CI. Configure target-specific Tauri settings for NSIS first (recommended per-user installer) or MSI if distribution needs justify it, plus a generated Windows icon and accurate platform description.

Choose WebView2 runtime installation policy deliberately: an online bootstrapper adds an installation-time download; an offline runtime option increases package size. Disclose this distinction from Lyra's default-off application network features. Code-sign binary/installer, timestamp signatures and protect signing credentials; unsigned internal artifacts are not the signed release acceptance. [Tauri Windows installers](https://v2.tauri.app/distribute/windows-installer/).

Add Windows dependency auditing in `deny.toml` and exact-version notices. Shared crate tests run on both OSes; platform sink behavior uses a mocked Windows API adapter in CI. Native desktop smoke tests use WebView2 and a matching WebDriver on Windows, replacing Linux Xvfb/WebKitGTK assumptions. Neither VM/CI audio nor WASAPI loopback proves exclusive-mode bit-perfect output.

## 9. Phased implementation checklist

### M6-W0 — Freeze contracts and baseline

- [ ] Finish required Linux M5 acceptance; record fixture/scan/stream behavior before refactoring.
- [ ] Adopt the Windows 11 x86_64 target and WASAPI-first scope in the main spec/plan.
- [ ] Decide shared endpoint/report/volume contracts, path codec, cancellation and clock semantics.
- [ ] List candidate Windows bindings/image dependencies and licences. Investigate ASIO terms separately.

**Gate:** reviewed design and dated Linux baseline; no Windows audio claims.

### M6-W1 — Portable workspace and desktop shell

- [ ] Target-gate Linux dependencies/modules/examples/tests so Windows workspace checks are meaningful.
- [ ] Implement lossless path/timestamp migration and missing-storage preservation tests.
- [ ] Add platform facade, inert backend factory, portable artwork and Windows protocol/CSP handling.
- [ ] Add native Windows CI/checks and a shell build with a mock sink; folder scan, library, playlists and artwork work.

**Gate:** formatting, Clippy and workspace tests pass on Linux and Windows; frontend checks/build and native shell smoke pass; Linux playback fixture results unchanged. No real Windows sink required yet.

### M6-W2 — WASAPI exclusive PCM

- [ ] Implement thread-owned COM resources, enumeration, exact negotiation, event-driven writes and preallocated packet staging.
- [ ] Implement cancellable pause/seek/discard/drain/stop/shutdown and submitted/rendered position reporting.
- [ ] Add format/refusal, full-buffer boundary, clock, cancellation and error mapping tests using a mock API.
- [ ] Integrate common diagnostics and selected endpoint preference.

**Gate:** synthetic 16/44.1 and 24/96 PCM plays on a physical Windows DAC with requested/initialized format records and payload evidence where measurable; unsupported layouts never fall back; no software gain or hidden shared mode.

### M6-W3 — DoP and volume integrity

- [ ] Gate DoP by carrier support, confirmed DAC capability and verified volume behavior.
- [ ] Add continuous marker/packet/tail tests, gapless and format-change handling; explicit conversion behavior matches existing opt-in policy.
- [ ] Verify DAC/driver at DSD64 and DSD128 where supported; record DSD256 refusal if the carrier is unavailable.
- [ ] Test external gain/mute changes and unknown/non-DSD outputs without audible auto-probing.

**Gate:** DoP byte tests pass and physical transport evidence is recorded; a DSD lamp alone is supporting evidence. Unverified combinations remain restricted and clearly labelled.

### M6-W4 — Desktop behavior and resilience

- [ ] Add endpoint notifications, SMTC, volume callbacks, sleep/resume and driver-reset handling.
- [ ] Validate device preference/reconnect policy, stale endpoints and exclusive access disabled/busy messages.
- [ ] Exercise large libraries and scan/metadata workloads during playback; record UI timing, memory and glitches.
- [ ] Complete WebView2 accessibility/DPI/titlebar/artwork tests.

**Gate:** matrix below passes for promised behaviors; stop and shutdown have measured bounded completion.

### M6-W5 — Distribution and acceptance

- [ ] Configure/sign Windows installer, choose/test WebView2 policy, audit dependencies and regenerate notices.
- [ ] Test clean install, upgrade, uninstall/data retention and non-admin operation on a real Windows machine.
- [ ] Publish Windows development/user docs and per-DAC hardware reports, with explicit unsupported cases.

**Gate:** signed installer plus recorded PCM/DoP acceptance, Linux regression checks passing. This completes Windows WASAPI scope only, not all of M6 or ASIO/macOS.

### M6-W6 — Optional ASIO/native DSD

- [ ] Complete SDK/binding/licence and vendor-driver review before integration.
- [ ] Implement callback-safe backend and runtime native-DSD packing/capability selection.
- [ ] Validate DSD rates and transitions on physical hardware with exact driver records.

**Gate:** approved distribution path and independent native-DSD acceptance; otherwise leave feature absent with no impact on WASAPI release.

## 10. Acceptance matrix and hardware report

| ID  | Scenario                                                              | Expected evidence                                                                              |
| --- | --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| W01 | PCM 16/44.1, 24/96 and supported 32-bit integer                       | Exact initialized format/channel map; fixture bytes and Windows hardware-path evidence         |
| W02 | Unsupported rate/layout, exclusive disabled or busy                   | Actionable refusal, no shared/default-device/rate fallback                                     |
| W03 | DoP DSD64/128                                                         | Continuous markers/channel order; carrier format; compatible DAC behavior and payload evidence |
| W04 | DSD256 on endpoint limited to 384 kHz; Native-only on WASAPI          | Explicit transport explanation; opt-in conversion only under Auto when eligible                |
| W05 | Unknown speakers or external hardware attenuation/mute                | Refuse unverified DoP; controlled stop on threatening change; verdict reflects actual path     |
| W06 | Same-format PCM and DoP tracks                                        | No stream reopen or added boundary silence; correct partial-packet and marker handling         |
| W07 | Seek/pause/stop/quit during blocked write/drain                       | No stale-generation frames; measured cancellation and cleanup; proper tail behavior            |
| W08 | Unplug/replug, default-device change, sleep/resume                    | Controlled stop/error; no silent output migration or auto-resume                               |
| W09 | Unicode/long/case-sensitive paths, removable roots and reparse points | Correct identity/scoping; no loss of playlists/edits on incomplete scans                       |
| W10 | 10k-track scan/artwork/metadata jobs during playback                  | Recorded responsiveness, disk/RAM/CPU load and glitch/underrun outcome                         |
| W11 | Media keys, SMTC, WebView2, titlebar/DPI/accessibility                | Correct state/metadata and single control action; artwork loads under restricted CSP           |
| W12 | Signed install/upgrade/uninstall, WebView2 presence/absence           | Verifiable signature, expected privileges/runtime install behavior, user data policy           |
| W13 | Optional native ASIO DSD                                              | Licence approval, driver sample types/rates and DSD payload/hardware verification              |
| W14 | Linux regression after each shared change                             | Existing fixture suites, library preservation, Rust/frontend/native Linux checks               |

Hardware reports in `docs/hardware/` should record date, Windows edition/build/architecture, DAC model/firmware/USB IDs, vendor/class-driver name/version, selected backend and endpoint, source fixture digest, requested/initialized format, valid/container bits, channel mask, period/buffer sizes, output/clock observations, volume/mute classification, glitches, transitions and measured outcome. Redact host/user details and use an endpoint-ID digest in public reports if appropriate.

Host API negotiation plus a DSD indicator is insufficient for an unrestricted bit-perfect claim. Use deterministic output-byte comparisons and a documented capture/measurement method where available; state when physical payload capture is unavailable and restrict claims to the evidence collected. Linux hardware results do not establish Windows behavior.

## 11. Effort, risks and unresolved decisions

Indicative effort for one developer familiar with the repository, assuming Linux M5 is stable:

| Work                                                            | Indicative effort                        |
| --------------------------------------------------------------- | ---------------------------------------- |
| Contracts, portability and native shell/CI (W0–W1)              | 1–2 developer weeks                      |
| WASAPI PCM + DoP implementation/tests (W2–W3)                   | 2–4 weeks                                |
| Desktop/resilience, hardware evidence, signed packaging (W4–W5) | 1–3 weeks                                |
| Optional ASIO/native DSD (W6)                                   | 2–4+ weeks after licensing/driver access |

WASAPI release scope is roughly **4–9 developer weeks**, with hardware/signing access and driver investigation able to extend calendar time. Re-estimate after W1/W2. ASIO approval timing is outside engineering control.

Main risks: device-specific DoP support, Windows hardware volume semantics, driver packet/alignment behavior, blocking cancellation, source/clock latency mapping, filesystem identity, WebView2 differences and ASIO licensing. Before coding, confirm available Windows machine/DAC/driver combinations, signing ownership and installer/runtime policy. The design proposes defaults so that these questions do not require changes to the current working tree.
