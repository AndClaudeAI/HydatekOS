#!/usr/bin/env bash
# Build a bootable HydatekOS disk image (GPT + EFI System Partition).
#
#   tools/mkimage.sh            -> build/hydatekos.img
#
# Write the image to a USB stick (all data on it is erased!):
#   Linux/macOS: sudo dd if=build/hydatekos.img of=/dev/sdX bs=4M status=progress
#   Windows:     use Rufus or balenaEtcher in "DD image" mode
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/build"
IMG="$OUT/hydatekos.img"
SIZE_MB=${SIZE_MB:-128}

command -v mformat >/dev/null || { echo "mtools is required (apt install mtools / brew install mtools)"; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required"; exit 1; }

(cd "$ROOT/kernel" && cargo build --release)
EFI="$ROOT/kernel/target/x86_64-unknown-uefi/release/hydatek.efi"

mkdir -p "$OUT"
rm -f "$IMG"
dd if=/dev/zero of="$IMG" bs=1M count="$SIZE_MB" status=none
# One EFI System Partition starting at 1 MiB.
python3 "$ROOT/tools/mkgpt.py" "$IMG"
PART="$IMG@@1M"
mformat -i "$PART" -T $(( (SIZE_MB - 2) * 2048 )) -F -v HYDATEKOS ::
mmd -i "$PART" ::/EFI ::/EFI/BOOT ::/HYDATEK
mcopy -i "$PART" "$EFI" ::/EFI/BOOT/BOOTX64.EFI
echo "HydatekOS image: $IMG"
