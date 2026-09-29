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
# ARM64 (Qualcomm Snapdragon and other ARM laptops): the same stick boots
# both. Skipped when the target isn't installed (rustup target add
# aarch64-unknown-uefi), unless ARM64=1 insists.
EFI_ARM=""
if [ "${ARM64:-auto}" != 0 ] && rustup target list --installed 2>/dev/null | grep -q aarch64-unknown-uefi; then
  (cd "$ROOT/kernel" && cargo build --release --target aarch64-unknown-uefi)
  EFI_ARM="$ROOT/kernel/target/aarch64-unknown-uefi/release/hydatek.efi"
elif [ "${ARM64:-auto}" = 1 ]; then
  echo "ARM64=1 needs: rustup target add aarch64-unknown-uefi"; exit 1
else
  echo "note: rustup target add aarch64-unknown-uefi to include ARM64 (Snapdragon)"
fi

mkdir -p "$OUT"
rm -f "$IMG"
dd if=/dev/zero of="$IMG" bs=1M count="$SIZE_MB" status=none
# One EFI System Partition starting at 1 MiB.
python3 "$ROOT/tools/mkgpt.py" "$IMG"
PART="$IMG@@1M"
mformat -i "$PART" -T $(( (SIZE_MB - 2) * 2048 )) -F -v HYDATEKOS ::
mmd -i "$PART" ::/EFI ::/EFI/BOOT ::/HYDATEK
mcopy -i "$PART" "$EFI" ::/EFI/BOOT/BOOTX64.EFI
[ -n "$EFI_ARM" ] && mcopy -i "$PART" "$EFI_ARM" ::/EFI/BOOT/BOOTAA64.EFI
# Phone Link's Android app, served to phones at http://<pc>:7743/app.apk
APK="$ROOT/companion/android/build/hydatek-link.apk"
if [ -f "$APK" ]; then
  mmd -i "$PART" ::/HYDATEK/apps
  mcopy -i "$PART" "$APK" ::/HYDATEK/apps/hydatek-link.apk
else
  echo "note: build companion/android/build.sh to include the Android app"
fi
echo "HydatekOS image: $IMG"
