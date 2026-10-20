#!/usr/bin/env python3
"""Generate matching DSF and DFF test files from one synthetic DSD signal.

Default: small fixtures in crates/format-dsd/tests/fixtures (committed).
--seconds N --out DIR: longer files for listening tests (not committed).

The signal is a 1 kHz tone through a second-order sigma-delta modulator,
DSD64 stereo (right channel inverted). The DFF data chunk holds the bytes in
Lyra's canonical order (MSB first, interleaved per byte), so it is the
reference both readers must reproduce. The DSF copy stores the same bits LSB
first in 4096-byte blocks, with a sample count that ends mid-byte and a
zero-padded last block.
"""

import argparse
import math
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RATE = 2_822_400
BLOCK = 4096


def modulate(bits: int, phase: float) -> bytes:
    """Second-order sigma-delta: returns MSB-first packed bits."""
    out = bytearray()
    i1 = i2 = 0.0
    byte = 0
    for n in range(bits):
        x = 0.5 * math.sin(2 * math.pi * 1000 * n / RATE + phase)
        y = 1.0 if i2 >= 0 else -1.0
        i1 += x - y
        i2 += i1 - y
        byte = (byte << 1) | (y > 0)
        if n % 8 == 7:
            out.append(byte)
            byte = 0
    return bytes(out)


def interleave(channels: list[bytes]) -> bytes:
    return bytes(b for frame in zip(*channels) for b in frame)


def dff(channels: list[bytes]) -> bytes:
    def chunk(cid: bytes, body: bytes) -> bytes:
        return cid + struct.pack(">Q", len(body)) + body + (b"\0" if len(body) % 2 else b"")

    count = len(channels)
    ids = [b"SLFT", b"SRGT"][:count]
    prop = b"SND " + chunk(b"FS  ", struct.pack(">I", RATE)) + chunk(
        b"CHNL", struct.pack(">H", count) + b"".join(ids)
    ) + chunk(b"CMPR", b"DSD " + bytes([14]) + b"not compressed" + b"\0")
    body = b"DSD " + chunk(b"FVER", struct.pack(">I", 0x01050000)) + chunk(b"PROP", prop) + chunk(
        b"DSD ", interleave(channels)
    )
    return b"FRM8" + struct.pack(">Q", len(body)) + body


def dsf(channels: list[bytes], sample_count: int) -> bytes:
    count = len(channels)
    frames = len(channels[0])
    groups = -(-frames // BLOCK)
    data = bytearray()
    for g in range(groups):
        for ch in channels:
            block = bytes(int(f"{b:08b}"[::-1], 2) for b in ch[g * BLOCK:(g + 1) * BLOCK])
            data += block + bytes(BLOCK - len(block))
    fmt = b"fmt " + struct.pack("<QIIIIIIQII", 52, 1, 0, 2 if count == 2 else 1, count, RATE, 1, sample_count, BLOCK, 0)
    data_chunk = b"data" + struct.pack("<Q", 12 + len(data)) + bytes(data)
    total = 28 + len(fmt) + len(data_chunk)
    return b"DSD " + struct.pack("<QQQ", 28, total, 0) + fmt + data_chunk


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--seconds", type=float, default=0.0)
    parser.add_argument("--out", type=Path, default=ROOT / "crates/format-dsd/tests/fixtures")
    args = parser.parse_args()
    # Default: two full blocks and part of a third, ending mid-byte.
    frames = round(RATE / 8 * args.seconds) if args.seconds else 2 * BLOCK + 1234
    bits = frames * 8 - 3
    left = modulate(frames * 8, 0.0)
    right = bytes(b ^ 0xFF for b in left)
    channels = [left, right]
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "dsd64_stereo.dff").write_bytes(dff(channels))
    (args.out / "dsd64_stereo.dsf").write_bytes(dsf(channels, bits))
    print(f"{args.out}/dsd64_stereo.{{dsf,dff}}: {frames} frames, DSD64 stereo")
    return 0


if __name__ == "__main__":
    sys.exit(main())
