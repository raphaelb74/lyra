# iFi ZEN ONE — hardware report

Status: **descriptor evidence only.** Nothing below has been confirmed by playback yet; that is M1 (PCM) and M2/M3 (DoP, native DSD).

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

## Bit-perfect risks found

1. **USB volume control.** The device exposes hardware volume (`'ZEN ONE ',0`, 0–32512). It was at 32511 = **−0.01 dB**, not 0 dB; PipeWire had set it. If the XMOS firmware applies this digitally, any setting below maximum alters samples. Lyra must read this control and either set it to maximum or show "bit-perfect: no (volume −0.01 dB)". Verify with a loopback or the DAC's own indicator.
2. **PipeWire owns a sink for it** (`ZEN ONE Analog Stereo`, volume 0.40, not the default sink). The PCM was `closed` at probe time, so Lyra can open `hw:` directly while nothing plays to that sink. If an app starts playing there, WirePlumber holds the device and Lyra must report "busy" (A06).

## Still to verify

- [ ] PCM 16/44.1 and 24/96 play bit-perfectly through `hw:CARD=ONE,DEV=0` (M1).
- [ ] Rate switching between 44.1 and 48 kHz families without clicks.
- [ ] Behaviour of the hardware volume control (digital or analog).
- [ ] DoP DSD64/128 recognised (M2); native DSD64/128/256 (M3).
- [ ] Unplug mid-track → controlled error (A05).
