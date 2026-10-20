#!/usr/bin/env python3
"""Generate deterministic PCM test files (WAV + FLAC) for bit-perfect checks.

Default: short fixtures in crates/format-pcm/tests/fixtures (committed).
--seconds N --out DIR: longer files for listening/hardware tests (not committed).

Signal: a sine plus pseudo-random noise, with the format's extreme values in the
first frames. FLAC files are encoded from the WAV with the `flac` tool, so the
WAV data chunk is the reference for every decoded sample.
"""

import argparse
import math
import shutil
import subprocess
import sys
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORMATS = [(16, 44100), (24, 96000)]


def samples(bits: int, rate: int, frames: int):
    peak = (1 << (bits - 1)) - 1
    lowest = -(1 << (bits - 1))
    state = 0x1234_5678
    yield from [(lowest, peak), (peak, lowest), (0, -1), (-1, 0)]
    for n in range(frames - 4):
        state = (state * 1_103_515_245 + 12_345) & 0x7FFF_FFFF
        noise = (state >> 8) % (peak // 8) - peak // 16
        tone = round(peak * 0.5 * math.sin(2 * math.pi * 997 * n / rate))
        yield tone + noise, -tone + (noise >> 1)


def write_wav(path: Path, bits: int, rate: int, frames: int) -> None:
    width = bits // 8
    data = bytearray()
    for left, right in samples(bits, rate, frames):
        data += left.to_bytes(width, "little", signed=True)
        data += right.to_bytes(width, "little", signed=True)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(width)
        out.setframerate(rate)
        out.writeframes(bytes(data))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--seconds", type=float, default=0.1)
    parser.add_argument("--out", type=Path, default=ROOT / "crates/format-pcm/tests/fixtures")
    args = parser.parse_args()
    if shutil.which("flac") is None:
        print("error: `flac` not found (sudo apt install flac)", file=sys.stderr)
        return 1
    args.out.mkdir(parents=True, exist_ok=True)
    for bits, rate in FORMATS:
        stem = args.out / f"s{bits}_{rate}"
        wav = stem.with_suffix(".wav")
        write_wav(wav, bits, rate, round(rate * args.seconds))
        # --no-padding keeps the files minimal; -8 exercises
        # the predictors. FLAC stores the MD5 of the WAV samples.
        subprocess.run(
            ["flac", "--silent", "--force", "-8", "--no-padding", "-o", str(stem.with_suffix(".flac")), str(wav)],
            check=True,
        )
        print(f"{wav.relative_to(ROOT) if wav.is_relative_to(ROOT) else wav} (+ .flac)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
