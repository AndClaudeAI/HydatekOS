#!/usr/bin/env python3
"""Write a GPT with a single EFI System Partition (1 MiB .. end-1 MiB) to a
disk image, without needing root, sfdisk or gdisk. Usage: mkgpt.py IMAGE"""
import struct, sys, uuid, zlib

SECTOR = 512
ESP = uuid.UUID("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")


def main(path):
    with open(path, "r+b") as f:
        f.seek(0, 2)
        total = f.tell() // SECTOR
        first, last = 2048, total - 2048
        # protective MBR
        mbr = bytearray(SECTOR)
        mbr[446:462] = struct.pack("<BBBBBBBBII", 0, 0, 2, 0, 0xEE, 0xFF, 0xFF, 0xFF, 1, min(total - 1, 0xFFFFFFFF))
        mbr[510:512] = b"\x55\xaa"
        # partition entry array (128 entries x 128 bytes)
        entries = bytearray(128 * 128)
        name = "HYDATEKOS".encode("utf-16-le")
        entries[0:128] = struct.pack("<16s16sQQQ72s", ESP.bytes_le, uuid.uuid4().bytes_le, first, last, 0, name.ljust(72, b"\0"))
        ecrc = zlib.crc32(entries) & 0xFFFFFFFF
        disk = uuid.uuid4().bytes_le

        def header(cur, backup, entries_lba):
            h = struct.pack("<8sIIIIQQQQ16sQIII", b"EFI PART", 0x10000, 92, 0, 0, cur, backup, 34, total - 34, disk, entries_lba, 128, 128, ecrc)
            crc = zlib.crc32(h) & 0xFFFFFFFF
            h = h[:16] + struct.pack("<I", crc) + h[20:]
            return h.ljust(SECTOR, b"\0")

        f.seek(0); f.write(mbr)
        f.seek(SECTOR); f.write(header(1, total - 1, 2))
        f.seek(2 * SECTOR); f.write(entries)
        f.seek((total - 33) * SECTOR); f.write(entries)
        f.seek((total - 1) * SECTOR); f.write(header(total - 1, 1, total - 33))


if __name__ == "__main__":
    main(sys.argv[1])
