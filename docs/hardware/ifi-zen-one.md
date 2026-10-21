# iFi ZEN ONE — hardware report

Status: **PCM verified (M1); DoP DSD64 and DSD128 verified (M2); native DSD64, DSD128 and DSD256 verified (M3)**, see [Playback results](#playback-results-m1), [DoP](#dop-m2) and [Native DSD](#native-dsd-m3).

Collected 2026-10-20 with `scripts/probe-dac.sh` (read-only; the PCM device was never opened).

## Identity

| Field | Value |
| --- | --- |
| Product | iFi (by AMR) ZEN ONE |
| USB ID | `20b1:3009` (XMOS USB audio controller) |
| Device release (`bcdDevice`) | 1.0c (firmware version string not exposed over USB) |
| USB | 2.0 high speed, asynchronous endpoint with feedback (`0x81 IN`) |
| ALSA card | `card 2: ONE [ZEN ONE]`, device 0 → `hw:CARD=ONE,DEV=0` |
| Host | Linux Mint 22.3, kernel 7.2.6-070206-generic, ALSA driver k7.2.6, alsa-utils 1.2.9, PipeWire 1.0.5 |

Card indices change with plug order; the stable name is `hw:CARD=ONE,DEV=0`.

## Formats exposed by the driver (`/proc/asound/card2/stream0`)

| Altset | ALSA format | Container | Rates (Hz) |
| --- | --- | --- | --- |
| 1 | `S32_LE` | 32-bit | 44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000 |
| 2 | `S24_3LE` | packed 24-bit | same |
| 3 | `S16_LE` | 16-bit | same |
| 4 | `DSD_U32_BE` | native DSD, 32 bits per channel per frame | same |

All stereo (`FL FR`). The driver reports `DSD raw: DOP=0, bitrev=0`: native DSD, no kernel-side DoP, no bit reversal.

## What that implies (to verify by playback)

**PCM:** 16/44.1 and 24/96 (the M1 acceptance files) map directly to `S16_LE` / `S24_3LE` (or `S32_LE`) at their native rate. No resampling or format conversion should be needed.

**Native DSD** (`DSD_U32_BE`: one ALSA frame carries 32 DSD bits per channel, so DSD bit rate = frame rate × 32):

| Source | DSD bit rate | ALSA frame rate | Listed? |
| --- | --- | --- | --- |
| DSD64 | 2.8224 MHz | 88200 | yes |
| DSD128 | 5.6448 MHz | 176400 | yes |
| DSD256 | 11.2896 MHz | 352800 | yes |
| DSD64/128/256 (48 kHz family) | — | 96000 / 192000 / 384000 | yes |

The 44100/48000 entries (DSD32) are probably an artefact of the driver sharing one rate list across altsets; don't offer them until playback proves them.

**DoP** (DSD bit rate / 16 on a 24-bit PCM carrier): DSD64 → 176.4 kHz and DSD128 → 352.8 kHz fit; **DSD256 needs 705.6 kHz, which this device does not list**, so DSD256 is native-only here (spec A04).

## Playback results (M1)

2026-10-20, Lyra `audio-alsa` `play` example (`cargo run -p audio-alsa --example play -- hw:CARD=ONE,DEV=0 <file>`), direct `hw:` open, PipeWire running. USB volume set to 32512 (0 dB) first.

| File | Decode check | Lyra output | Kernel `hw_params` (during playback) | Buffer / period | Underruns |
| --- | --- | --- | --- | --- | --- |
| FLAC 16/44.1, 30 s | MD5 matches | `S16_LE`, 44100, 2 ch | `S16_LE`, 44100 (44100/1), 2 ch | 11025 / 2756 frames | 0 |
| FLAC 24/96, 30 s | MD5 matches | `S24_3LE`, 96000, 2 ch | `S24_3LE`, 96000 (96000/1), 2 ch | 24000 / 6000 frames | 0 |
| FLAC 24/48, 30 s | MD5 matches | `S24_3LE`, 48000, 2 ch | `S24_3LE`, 48000 (48000/1), 2 ch | 12000 / 3000 frames | 0 |

The three files played back to back (44.1 → 96 → 48 kHz), each reopening the device at the file's own rate.

What this proves: the decoder reproduces the encoder's samples (FLAC MD5), Lyra writes them unchanged (byte-exact tests against the mock sink), and the kernel runs the device at the source rate and sample format with no plug layer. Not proven: what the DAC firmware does after USB (needs a digital loopback or measurement).

**Busy device (A06):** with PipeWire playing to the ZEN ONE sink, Lyra reported `hw:CARD=ONE,DEV=0 is in use by pipewire (pid 1283)` and did not fall back to another device. To free the DAC for Lyra, switch its PipeWire profile off, and back on afterwards:

```sh
pactl set-card-profile alsa_card.usb-iFi__by_AMR__ZEN_ONE_0025-00 off
pactl set-card-profile alsa_card.usb-iFi__by_AMR__ZEN_ONE_0025-00 output:analog-stereo
```

(`pactl list cards short` lists card names on other systems.) PipeWire only holds the device while something plays to it; it releases it a few seconds after playback stops.

**Unplug mid-track (A05):** pulling the USB cable 6 s into the 24/96 file gave `DeviceNotFound: hw:CARD=ONE,DEV=0 was disconnected`; the player stopped cleanly, without a crash, hang, or switch to another device.

## DoP (M2)

2026-10-20, Lyra desktop app, DSF DSD64 stereo from the user's library, PipeWire running.

| Source | Lyra output | Kernel `hw_params` (during playback) | Buffer / period | Device owner | USB volume |
| --- | --- | --- | --- | --- | --- |
| DSF DSD64, 2 ch | DoP, `S24_3LE`, 176400, 2 ch | `S24_3LE`, 176400 (176400/1), 2 ch, `RUNNING` | 44100 / 11025 frames | `lyra-writer` | 32512 (0 dB) |
| DSF DSD128, 2 ch | DoP, `S24_3LE`, 352800, 2 ch | `S24_3LE`, 352800 (352800/1), 2 ch, `RUNNING` | 88200 / 22050 frames | `lyra-writer` | 32512 (0 dB) |

The user confirmed playback on the DAC, and the ZEN ONE's format LED changes colour to its DSD indication: the DAC recognises the DoP markers (supporting evidence, not proof of bit-exactness). The engine tests prove the DoP stream carries every DSD bit of the file with alternating `0x05`/`0xFA` markers (DSF and DFF); DoP is refused unless the USB volume is at 0 dB or absent.

## Native DSD (M3)

2026-10-21, `audio-alsa` `play` example (release build), transport `auto` unless noted, PipeWire running with the card idle.

| Source | Transport | Lyra output | Kernel `hw_params` (during playback) | Underruns | Device owner | USB volume |
| --- | --- | --- | --- | --- | --- | --- |
| DSF DSD64, 2 ch (library) | auto → native | `DSD_U32_BE`, 88200, 2 ch | `DSD_U32_BE`, 88200 (88200/1), 2 ch | 0 | `lyra-writer` | 0 dB (Unity) |
| DSF DSD128, 2 ch (library) | auto → native | `DSD_U32_BE`, 176400, 2 ch | `DSD_U32_BE`, 176400 (176400/1), 2 ch | — (stopped early) | `lyra-writer` | 0 dB |
| DSF DSD256, 2 ch (synthetic, 10 s) | auto → native | `DSD_U32_BE`, 352800, 2 ch | `DSD_U32_BE`, 352800 (352800/1), 2 ch | 0 (full file) | `lyra-writer` | 0 dB |
| DFF DSD256, 2 ch (synthetic, 10 s) | auto → native | `DSD_U32_BE`, 352800, 2 ch | `DSD_U32_BE`, 352800 (352800/1), 2 ch | 0 (full file) | `lyra-writer` | 0 dB |
| DSF DSD64, 2 ch (library) | `dop` | DoP, `S24_3LE`, 176400 | `S24_3LE`, 176400 (176400/1) | — | `lyra-writer` | 0 dB |
| DSF DSD256 (synthetic) | `dop` | refused: `cannot play this: DSD256 as DoP at 705600 Hz` | not opened | — | — | — |

With `auto`, Lyra picks native DSD on this DAC, since the driver exposes `DSD_U32_BE` (`DSD raw: DOP=0, bitrev=0`). DSD256 is native-only here, as predicted from the descriptors (spec A04): forcing DoP fails with an explicit message, never a PCM fallback. The engine tests prove the native stream carries every DSD bit of the file in `DSD_U32_BE` order (DSF and DFF), with the last word filled with DSD silence (`0x69`). The USB volume was restored to 32511 after the runs.

## Bit-perfect risks found

1. **USB volume control.** The device exposes hardware volume (`'ZEN ONE ',0`, 0–32512). It was at 32511 = **−0.01 dB**, not 0 dB; PipeWire had set it. The ZEN ONE has no volume knob: this control is only visible to software. If the XMOS firmware applies it digitally, any setting below maximum alters samples. Set to 0 dB with `amixer -c ONE cset numid=4 32512,32512 && amixer -c ONE cset numid=5 32512`; it stayed at 0 dB after PipeWire used the device again, but **after unplugging and replugging it was back at 32511**, so setting it once is not enough. **Lyra now handles it** (`audio-alsa` `volume.rs`): on USB Audio cards, a control within 0.5 dB of 0 dB is set to exactly 0 dB while Lyra holds the device and restored on release (verified: 32511 → 32512 during playback → 32511 after). Lower settings are left alone and reported as attenuated (not bit-perfect), since on a headphone DAC they are the listening level.
2. **PipeWire owns a sink for it** (`ZEN ONE Analog Stereo`, volume 0.40, not the default sink). The PCM was `closed` at probe time, so Lyra can open `hw:` directly while nothing plays to that sink. If an app starts playing there, WirePlumber holds the device and Lyra must report "busy" (A06).

## Still to verify

- [x] PCM 16/44.1, 24/48 and 24/96 open at the exact format and rate through `hw:CARD=ONE,DEV=0`, no underruns (M1).
- [x] Busy device reported with the holding process (A06).
- [x] Rate switching between 44.1 and 48 kHz families: no audible clicks (listening test).
- [ ] Behaviour of the hardware volume control (digital or analog).
- [x] DoP DSD64 plays at the 176.4 kHz carrier; the DAC indicates DSD (M2).
- [x] DoP DSD128 plays at the 352.8 kHz carrier; the DAC indicates DSD (M2).
- [x] Native DSD64/128/256 play as `DSD_U32_BE` at 88.2/176.4/352.8 kHz (M3). In the desktop app, DSD128 shows `Native DSD · DSD_U32_BE · 176.4 kHz`; the user confirmed correct playback and the DSD indication.
- [x] Unplug mid-track → controlled error (A05).
