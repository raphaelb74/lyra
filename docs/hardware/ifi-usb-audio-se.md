# iFi USB Audio SE — hardware report

Status: **PCM 16/44.1 and 24/96 and native DSD64 verified by playback; DoP refusal at an attenuated volume verified.** DoP itself, DSD128/256 and bit-perfect playback at 0 dB are not yet verified (the volume was left where the user had it).

Collected 2026-10-21 with `scripts/probe-dac.sh` (read-only) and the `audio-alsa` `play` example.

## Identity

| Field | Value |
| --- | --- |
| Product | iFi USB Audio SE (as reported over USB) |
| USB ID | `2fc6:f082` |
| Device release (`bcdDevice`) | 2.01 |
| USB | 2.0 high speed, asynchronous endpoint with feedback (`0x81 IN`) |
| ALSA card | `card 2: SE [iFi USB Audio SE]`, device 0 → `hw:CARD=SE,DEV=0` |
| Host | Linux Mint 22.3, kernel 7.2.6-070206-generic, ALSA driver k7.2.6, alsa-utils 1.2.9, PipeWire 1.0.5 |

Card indices change with plug order; the stable name is `hw:CARD=SE,DEV=0`.

## Formats exposed by the driver (`/proc/asound/card2/stream0`)

| Altset | ALSA format | Container | Rates (Hz) |
| --- | --- | --- | --- |
| 1 | `S16_LE` | 16-bit | 44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000 |
| 2 | `S24_3LE` | packed 24-bit | same |
| 3 | `S32_LE` | 32-bit | same |
| 4 | `DSD_U32_BE` | native DSD, 32 bits per channel per frame | same |

All stereo (`FL FR`); `DSD raw: DOP=0, bitrev=0`. The same layout as the ZEN ONE: native DSD64/128/256 at 88.2/176.4/352.8 kHz frame rates, DoP up to DSD128 (DSD256 over DoP would need 705.6 kHz).

## Volume controls

Two playback controls, applied in series:

| Control | Channels | Range | Setting during these tests |
| --- | --- | --- | --- |
| `PCM`,0 | Front Left, Front Right | 0–127 | 98, −14.50 dB |
| `PCM`,1 | Mono | 0–127 | 87, −20.00 dB |

Total: −34.5 dB. This finding changed Lyra: the bit-perfect check and the DoP refusal now add up controls in series (they reported only the lowest, −20 dB), and the in-app volume moves every control of a USB DAC together, so 100% means all at 0 dB.

## Playback results

Lyra `play` example, direct `hw:` open, PipeWire running, volume as above (not 0 dB: playback is exact on the wire but not bit-perfect after the DAC's digital volume).

| File | Lyra output | Kernel `hw_params` (during playback) | Buffer / period | Underruns |
| --- | --- | --- | --- | --- |
| FLAC 16/44.1 (fixture) | `S16_LE`, 44100, 2 ch | `S16_LE`, 44100 (44100/1) | 11025 / 2756 frames | 0 |
| FLAC 24/96 (fixture) | `S24_3LE`, 96000, 2 ch | `S24_3LE`, 96000 (96000/1) | 24000 / 6000 frames | 0 |
| DSF DSD64 (fixture), auto | native `DSD_U32_BE`, 88200 | `DSD_U32_BE`, 88200 (88200/1) | 22050 / 5513 frames | 0 |
| DSF DSD64 (fixture), `dop` | refused | — | — | — |

DoP refusal message: "DSD over DoP needs the DAC's volume at 0 dB; it is at −34.50 dB." Nothing was written to the device.

## Format switching

Reported by the user in the app: the first notes after a format change (PCM to DSD and back, or the first track after launch) sometimes came out as noise, and closing Lyra in the middle of a native DSD track could leave the DAC playing noise for whatever came next. From the command line, a clean or killed DSD stream followed by PCM through PipeWire played cleanly in every attempt, so the DAC needs time to switch, not a different shutdown.

Lyra's answer, as in other players (Roon's "resync delay"):
- **Silence before playback** when the device's format changes, and on the first open after start: 0.5 s by default, adjustable in Settings (Off to 2 s). Sent only after the output is accepted, so a refused DoP device never gets DoP silence.
- **A clean DSD ending:** stopped or closed mid-stream, Lyra drops the queued audio, plays 0.1 s of DSD idle pattern and lets it finish before releasing the device.

To confirm by ear on this DAC: PCM → native DSD → PCM in one session, and quitting during DSD, both without noise.

## To do

- With the volume at 0 dB (100% in Lyra): DoP DSD64/128 and the bit-perfect verdict.
- Native DSD128 and DSD256 from real files.
- A full album through the gapless queue (one device open, 0 underruns).
