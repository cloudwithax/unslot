#!/usr/bin/env python3
"""Makes the tiny disc images the CHD hashing tests read: a PlayStation disc and a Sega CD
disc, each as .cue/.bin and as .chd (with MAME's chdman). Run from this folder; only the
.chd files are kept, as the tests know the hash the .cue/.bin gives."""
import os
import struct
import subprocess
import tempfile

SYNC = b"\x00" + b"\xff" * 10 + b"\x00"


def bcd(n):
    return ((n // 10) << 4) | (n % 10)


def raw_sector(lba, data, mode):
    m, s, f = (lba + 150) // 4500, (lba + 150) // 75 % 60, (lba + 150) % 75
    header = SYNC + bytes([bcd(m), bcd(s), bcd(f), mode])
    if mode == 2:
        body = b"\x00\x00\x08\x00" * 2 + data + b"\x00" * 280  # form 1 subheader, no EDC/ECC
    else:
        body = data + b"\x00" * 288
    return header + body


def iso_sectors(files):
    with tempfile.TemporaryDirectory() as d:
        root = os.path.join(d, "root")
        os.mkdir(root)
        for name, content in files.items():
            with open(os.path.join(root, name), "wb") as f:
                f.write(content)
        iso = os.path.join(d, "disc.iso")
        subprocess.run(["genisoimage", "-quiet", "-o", iso, root], check=True)
        data = open(iso, "rb").read()
    return [data[i:i + 2048] for i in range(0, len(data), 2048)]


def write_disc(name, sectors, mode):
    raw = b"".join(raw_sector(i, s.ljust(2048, b"\0"), mode) for i, s in enumerate(sectors))
    open(name + ".bin", "wb").write(raw)
    # A short silent audio track after the data, as many games have.
    open(name + "-audio.bin", "wb").write(b"\x01\x02" * (2352 // 2) * 40)
    open(name + ".cue", "w").write(
        f'FILE "{name}.bin" BINARY\n  TRACK 01 MODE{mode}/2352\n    INDEX 01 00:00:00\n'
        f'FILE "{name}-audio.bin" BINARY\n  TRACK 02 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:02:00\n'
    )
    subprocess.run(["chdman", "createcd", "-f", "-i", name + ".cue", "-o", name + ".chd"],
                   check=True, capture_output=True)


exe = bytearray(b"PS-X EXE".ljust(2048, b"\0")) + bytes(range(256)) * 8
write_disc("ps1", iso_sectors({
    "SYSTEM.CNF": b"BOOT = cdrom:\\SLUS_123.45;1\r\nTCB = 4\r\n",
    "SLUS_123.45": bytes(exe),
}), 2)

header = bytearray(2048)
header[:16] = b"SEGADISCSYSTEM  "
header[0x100:0x110] = b"SEGA MEGA DRIVE "
write_disc("segacd", [bytes(header)] + [b"\0" * 2048] * 31, 1)
