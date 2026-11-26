#!/usr/bin/env python3
import math
import struct
import sys

RATE = 44100
NOTES = ((660.0, 0.16), (880.0, 0.24))
PEAK = 0.55
FADE = 0.012


def samples():
    out = []
    for freq, length in NOTES:
        count = int(RATE * length)
        fade = int(RATE * FADE)
        for n in range(count):
            level = PEAK * math.sin(2.0 * math.pi * freq * n / RATE)
            level *= min(1.0, n / fade, (count - 1 - n) / fade)
            decay = 1.0 - 0.35 * n / count
            out.append(int(round(level * decay * 32767)))
    return out


def aiff(values):
    pcm = b"".join(struct.pack(">h", v) for v in values)
    comm = struct.pack(">hIh", 1, len(values), 16) + struct.pack(">HQ", 0x400E, 0xAC44 << 48)
    body = b"AIFF"
    body += b"COMM" + struct.pack(">I", len(comm)) + comm
    ssnd = struct.pack(">II", 0, 0) + pcm
    body += b"SSND" + struct.pack(">I", len(ssnd)) + ssnd
    return b"FORM" + struct.pack(">I", len(body)) + body


def render():
    return aiff(samples())


def main(argv):
    if len(argv) != 2:
        sys.stderr.write("usage: gen_sound.py OUT.aiff\n")
        return 2
    with open(argv[1], "wb") as handle:
        handle.write(render())
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
