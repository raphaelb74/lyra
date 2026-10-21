# DSD→PCM fallback design (M4c)

Status: **implemented in M4c-1, 21 October 2026.** The proposal below is kept as written; [As built](#as-built) records what measurement and the code changed, and answers the open questions. The spec edits are done.

## Purpose

Some DACs and every built-in sound card cannot take DSD, either natively or as DoP. Today Lyra refuses those tracks. This design adds an **explicit, opt-in** way to play them anyway by converting DSD to PCM in software.

It is a last resort. It is never bit-perfect, it is never chosen silently, and it never touches the native/DoP path.

## Principles

1. **Native DSD, then DoP, then (only if the user allowed it) conversion.** Conversion is tried only after both bit-perfect transports have been ruled out by ALSA evidence.
2. **Opt-in.** A setting, "DSD on devices without DSD support: Refuse / Convert to PCM". The default stays **Refuse**, so existing behaviour and the existing `dsd_never_falls_back_to_pcm` test are unchanged.
3. **Always labelled.** A converted track is shown as "Converted to PCM, not bit-perfect", with the output rate, in every place that shows bit-perfect status.
4. **Off the time-critical path.** The conversion runs on the controller thread, which already decodes and packs audio. The writer thread only moves ready bytes, as it does now.
5. **No new dependencies.** The filter is designed with plain Rust and `std` at open time; nothing is generated or downloaded.

## Where it fits in the current code

| Today | With conversion |
| --- | --- |
| `StreamFormat::Dsd { dsd_rate, channels, transport }` describes what the source produces | Unchanged. Conversion is a way to carry the same source, not a different source |
| `StreamFormat::candidates()` lists native layouts, then DoP, as `DsdTransport` allows | Adds a third group, converted-PCM candidates, **only when the fallback setting is on**, always last |
| `OutputFormat::negotiate()` takes the first candidate the device accepts | Unchanged. Because converted candidates come last, they are used only if nothing before them is accepted |
| `Encoding::{Pcm, Dop, Dsd}` and `Transport::{Pcm, Dop, Native}` | One new case in `Transport` (for example `Converted`) so status and UI can say "not bit-perfect". The byte layout can reuse `Encoding::Pcm` |
| `Source::next()` fills a buffer with packed audio for the negotiated `Encoding` and returns the source frames read | A new DSD branch that runs the converter and packs 24-bit PCM. It still returns **source** frames (DSD bytes per channel) |
| `OutputFormat::carries()` decides gapless continuation | Two converted tracks at the same rate continue gaplessly; converted and non-converted never mix |

Positions, seeking and the writer's `position` counter all work in source frames (DSD bytes per channel, `dsd_rate / 8` per second). That does not change, so seek and progress code is untouched.

## Output rate

DSD rates are multiples of 44.1 kHz, so the PCM output stays in the 44.1 kHz family and every rate is an exact integer division of the DSD rate.

| Source | DSD rate | Output candidates (highest first) |
| --- | --- | --- |
| DSD64 | 2 822 400 Hz | 176 400, 88 200, 44 100 |
| DSD128 | 5 644 800 Hz | 352 800, 176 400, 88 200, 44 100 |
| DSD256 | 11 289 600 Hz | 352 800, 176 400, 88 200, 44 100 |
| DSD512 | 22 579 200 Hz | 352 800, 176 400, 88 200, 44 100 |

Rules:

- The candidate list is built from the DSD rate divided by 16, 32, 64… down to 44.1 kHz, and capped at 352.8 kHz. A larger cap buys nothing audible and costs bandwidth.
- Negotiation tries them in order and takes the **highest rate the device accepts**, as the plan says. No rate is invented: if the device accepts none of them, the track is refused with the usual message plus a note that conversion was tried.
- Sample layout reuses `SampleEncoding::lossless_for(24)`, so a 24-bit value goes into whichever container the device takes (32-bit or 24-bit packed).

## Filter approach

DSD is a 1-bit stream at a very high rate with the quantisation noise pushed far above the audio band. Converting it means low-pass filtering hard, then reducing the rate. The filter has to remove that ultrasonic noise **before** the rate drops, otherwise it folds back into the audio band.

Requirements, in order of importance:

1. **No aliasing.** Anything above the output Nyquist frequency is attenuated enough that it cannot fold into 0–20 kHz. Target: at least 120 dB of attenuation in the stopband.
2. **Flat audio band.** 20 Hz–20 kHz within ±0.05 dB.
3. **Linear phase.** A symmetric FIR filter, so no phase distortion. Latency is a fixed, known number of samples.
4. **Cheap enough to run well ahead of real time** on the controller thread (see [Performance](#performance)).

Structure, in plain words:

- **Stage 1** works directly on the bits. It is a long linear-phase low-pass that reduces the rate by a large integer factor (8 or 16). Each input byte holds eight 1-bit samples, so the contribution of eight filter taps to the output depends only on the byte's value. Those contributions are precomputed once into a table of 256 entries per group of eight taps, and each output sample is a sum of table lookups. This avoids per-bit multiplication.
- **Later stages** are conventional half-band decimate-by-2 filters, one per remaining halving, until the chosen output rate is reached. Half-band filters are cheap because nearly half their taps are zero.
- **Arithmetic** is `f64` throughout, which is cheap at these rates and leaves ample headroom over a 24-bit result. Coefficients are designed at open time (control plane) with a Kaiser-window method, using only `std`. Nothing is computed with allocation in the steady state.
- **Per channel.** Each channel has its own state (history of bits and half-band delay lines). Channels never mix.

Out of scope for the first version: user-selectable filter shapes (minimum phase, steep versus gentle roll-off). Audirvana exposes such options. They can come later, behind the same API, if listeners ask for them.

### Output word and dither

Converting `f64` to 24-bit integers loses information. The conversion uses TPDF dither at one 24-bit step, with a fixed-seed generator so tests are repeatable. Dither is part of why this path is labelled "not bit-perfect"; stating it plainly in the UI is the honest way to describe it.

## Level matching

The aim is that a converted track plays at the same loudness as the same recording would on a normal PCM release, and that it does not clip.

- **Proportional mapping.** A full-scale 1-bit stream (all ones) maps to +1.0 and all zeros to −1.0, so a DSD signal at −6 dB relative modulation comes out at −6 dBFS. No automatic per-track gain, so album levels stay consistent.
- **Headroom.** Real DSD masters can peak high, and the low-pass filter overshoots on transients, so the converted signal can exceed full scale even when the DSD stream is valid. The design therefore carries a **fixed, user-chosen reduction** (0, −3, −6 dB) and counts clipped samples.
- **Clipping is counted, not hidden.** Samples that would exceed full scale are clamped and counted. The count is shown in diagnostics (M4c) so a user can see if their reduction is too low.
- **Default.** The first implementation should pick the default from measurement, not from taste: scan a real library sample (including loud DSD masters), look at the overshoot, and choose the smallest reduction that keeps clipping at zero. This is an open question below.

## Placement on the controller thread

The existing split already fits:

- **Controller thread**: reads DSD bytes, runs the converter, packs 24-bit PCM into a pooled buffer, and sends the chunk to the writer. This is where decoding and DoP packing happen today, so no thread boundary moves.
- **Writer thread**: unchanged. It never sees DSD or filter code.

Consequences and rules:

- **Real-time margin.** The controller must stay ahead of the device. It fills the existing pool of buffers, so conversion has to run comfortably faster than real time, and the pool absorbs jitter.
- **One thread to start.** The first version runs all channels on the controller thread. Audirvana uses one worker per channel (observed in its `libdsdpcm.so` structure); that is a possible later optimisation if measurement shows a DSD512 stream cannot keep up. It is not built speculatively.
- **No allocation, I/O or logging in the conversion loop.** State and tables are sized at open time and reused.
- **Seek.** Reset filter state and discard the filter's start-up transient, so playback after a seek begins cleanly. Position stays in source frames.
- **Start and end of a track.** Compensate for the filter's fixed delay so the first output sample lines up with the first input, and flush the tail at the end of a track so the last samples are not lost.
- **Gapless.** When the next track is also converted at the same output rate, filter state carries over, as the DoP marker phase does today (`Source::continue_from`). That keeps the join free of a click. If the rate changes, the device is reopened as it is for any other format change.

## Settings and status

- Setting: "DSD on devices without DSD support" with **Refuse** (default) and **Convert to PCM**.
- Setting: reduction (0 / −3 / −6 dB), shown only when conversion is on.
- Setting: the existing DSD transport (Auto / Native / DoP). Conversion applies only after the chosen transport fails. If the user picks **Native** or **DoP only**, conversion does not override it; the refusal message says why.
- Status: transport shows "Converted to PCM" with the output rate, and the bit-perfect indicator is off for the whole track.
- Diagnostics: clipped-sample count and the chosen output rate and layout.

## Tests

Behaviour, not implementation details.

**Signal tests** (synthetic DSD from a small test-only delta-sigma modulator, so the expected result is known):

- A 1 kHz tone converts to a 1 kHz tone at the expected level (within ±0.1 dB) for DSD64, 128 and 256.
- Level mapping: a −6 dB modulation comes out near −6 dBFS with the reduction at 0 dB; each reduction step shifts it by exactly that amount.
- Silence (the DSD idle pattern) converts to near-zero output, with no DC offset beyond a stated bound.
- Aliasing: an ultrasonic test tone (for example 30 kHz and 60 kHz) is attenuated by the target amount at every output rate.
- Passband flatness across 20 Hz–20 kHz within the stated tolerance.
- Overshoot: a near-full-scale burst clamps, is counted, and does not wrap.

**Streaming tests:**

- Chunk invariance: converting the same input in one block and in many odd-sized blocks gives **bit-identical** output.
- Seek: after a seek the first output is the converted signal at the new position, with no start-up burst.
- Gapless: two same-format converted tracks join without a discontinuity larger than the signal itself.
- Channels stay independent: a tone on one channel leaves the other quiet.

**Policy tests** (extending those in `crates/player-core/src/sink.rs`):

- With **Refuse**, a DSD stream on a PCM-only device is still refused (the existing test keeps passing unchanged).
- With **Convert**, native is chosen when the device takes it, then DoP, then converted PCM; the converted candidate is the highest rate the mock accepts.
- Native-only and DoP-only never convert.
- `carries()` allows gapless between converted tracks only.
- Status shows "not bit-perfect" and the output rate.

**Controller tests:** with the mock sink, check that conversion output arrives as ordinary PCM chunks, positions stay in source frames, and the writer thread never runs conversion (the mock can record the thread it is called on).

**Hardware check:** play a converted track on the built-in card, record the result in `docs/hardware/`, and note that it is not claimed as bit-perfect.

## Performance

Targets to measure on the development machine, not assumed:

- DSD256 stereo converts at least 20× faster than real time on one thread.
- No heap allocation in the steady-state conversion loop (checked with a counting allocator in a test).
- Memory for the lookup tables stays within a few hundred KiB per channel.

If DSD512 misses its target, the next step is per-channel parallelism, decided from the measurement.

## Where the code goes

A new pure-computation crate, `crates/dsd-pcm`, with no I/O and no ALSA dependency. This keeps `format-dsd` about files and packing, matches the way `format-pcm` is separate, and lets the signal tests run without a device. The new `Source` branch in `player-core` calls it. This needs a workspace manifest change at implementation time, so it needs approval then.

## Spec changes

When this is implemented, update `docs/SPECIFICATION.md`:

- The non-goal list currently says "automatic DSD-to-PCM conversion". Change it to "**silent** DSD-to-PCM conversion", as the plan already anticipates.
- Section on DSD transport currently says "No automatic DSD→PCM fallback in v1". Replace it with the opt-in rule above.
- Add acceptance tests for the converted path, kept separate from the bit-perfect A-series.

## Open questions

1. **Default reduction.** 0 dB, −3 dB or −6 dB? Decide from a measured overshoot survey of real DSD files.
2. **Stage 1 factor and length.** Eight or sixteen, and how many taps, trade CPU against stopband depth. Measure both.
3. **Dither choice.** TPDF versus none. Dithering is standard, but a user may prefer plain rounding; consider an advanced toggle.
4. **Cap at 352.8 kHz.** Some DACs do not accept it; confirm the negotiation falls through to lower rates cleanly on the ZEN ONE.
5. **DSD512.** Is it worth supporting in the first version, or should it be refused until a performance measurement exists?

## Out of scope

- PCM→DSD upsampling.
- Choice of filter shapes.
- Volume control, EQ or any other DSP.
- Automatic switching to conversion when a transport fails mid-track. The choice is made once, at open, from ALSA evidence.

## As built

Code: `crates/dsd-pcm` (pure computation, no dependencies), the `Encoding::Converted` / `Transport::Converted` path in `player-core`, and the Settings view.

### Changes from the proposal

1. **DoP safety moved into negotiation.** Built-in audio accepts the DoP *format* (24-bit PCM at 176.4 kHz), so with conversion as the last candidate, DoP was chosen and then refused by the volume check, and conversion was never reached. `OutputFormat::negotiate` now takes the device volume: when conversion is allowed, DoP is skipped on a device whose volume cannot be shown to be untouched (`HardwareVolume::passes_dop`). With conversion off, DoP stays and the existing refusal explains why. The ALSA sink reads the volume before negotiating.
2. **Output at 88.2 kHz, 44.1 kHz fallback** (proposal: highest rate up to 352.8 kHz). The low-pass must remove DSD's shaped noise from about 30 kHz anyway, so 176.4 and 352.8 kHz would carry only filtered silence, while the steep final filter would cost several times more CPU at those rates.
3. **Gain, not reduction.** The survey below found no real track that clips at 0 dB, so the setting offers **0 dB (default), +3 dB and +6 dB**, toward PCM loudness. Clipped samples are clamped and counted; the converter also reports its peak, for diagnostics (M4c-2).
4. **The tail is flushed only at a real end.** A converted track that continues gaplessly into another converted track at the same DSD rate hands over its filter state (`Source::continue_from`), so the two play as one stream. Otherwise the controller sends the filters' tail as a last chunk before draining.
5. **The bit filter is designed for 150 dB**, the others for 120 dB. The DSD idle pattern (0x69) repeats once per byte, exactly the first stage's output rate, so its leak lands on DC; at 120 dB that was about 8 LSB of 24-bit, at 150 dB it is below one.

### Filter

- Stage 1 over bits: decimates to 352.8 kHz (8 bits per output at DSD64, 16 at DSD128, …), lookup table of 256 entries per 8 taps.
- Then decimate-by-2 stages to the output rate. Every stage is a linear-phase Kaiser low-pass, flat to 20 kHz; intermediate stages stop what would fold into the band, the last stops everything from 30 kHz (from 22.05 kHz at 44.1 kHz).
- Filters are centred, so there is no delay to compensate: silence is assumed before the start, and fed after the end by `finish`.
- TPDF dither at one 24-bit step, fixed seed, reset on seek.

### Measured

| Check | Result |
|---|---|
| Filter specification (all stages, DSD64–DSD512, both rates) | Passband within 0.05 dB at 20 kHz; worst stopband point below −120 dB (bit stage below −150 dB) |
| 1 kHz tone, DSD64/128/256 | Level within 0.1 dB; everything else below −60 dB (the test modulator's own noise) |
| 35, 60, 100 kHz tones | Below −70 dB at the output (bounded by the test modulator) |
| Silence (0x69) | Dither only: every sample within ±1 step, mean within 0.05 step |
| Chunking, seek, gapless | Bit-identical to one-shot conversion, through the real controller and writer |
| Steady-state allocations | None (counting allocator, DSD64 and DSD256) |
| Speed, one thread, release | DSD64: 46× real time; DSD128: 35× (target 20×) |
| Peak survey: 124 tracks, 3 per album, 119 DSD64 + DSD128 | Peaks −14.8 to −2.4 dBFS, median −6.2; none clips at 0 dB |

The survey is reproducible: `cargo run --release -p dsd-pcm --example survey -- <folders>`.

One file in the survey, *Telegraph Road* (Dire Straits, *Love Over Gold*), is entirely 0x00 bytes: a broken rip. All zeros is full-scale negative DC in DSD, which converts to a constant −1.0 and clips. That is the correct conversion of what the file holds; a guard against such files (muting a stream stuck at one value) is a follow-up.

### Open questions, answered

1. **Default reduction:** none needed; the default gain is 0 dB (see above).
2. **Stage 1 factor:** decimate straight to 352.8 kHz; the table lookups make even DSD512 cheap.
3. **Dither:** TPDF, no toggle in this version.
4. **352.8 kHz cap:** replaced by the 88.2 kHz choice; the 44.1 kHz fallback is tested.
5. **DSD512:** supported; its filter meets the specification, and its steady state does not allocate. Its speed has not been measured on a real file (none in the test library).

### Not done here

- The writer-thread check ("the mock records the thread it is called on") was not built: conversion lives in `Source`, which only the controller thread owns, so the writer cannot reach it.
- Hardware check on the built-in card: pending, to be recorded in `docs/hardware/`.
- Showing the clip count and peak: M4c-2 diagnostics.

