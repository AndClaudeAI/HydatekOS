#!/bin/sh
# HydatekOS: start HydatekOS in QEMU (Mac: double-click; Linux: ./start-hydatekos.command)
cd "$(dirname "$0")" || exit 1
if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
  echo
  echo "  QEMU isn't installed yet."
  echo "  Mac:           brew install qemu"
  echo "  Ubuntu/Debian: sudo apt install qemu-system-x86"
  echo "  Fedora:        sudo dnf install qemu-system-x86"
  echo
  exit 1
fi
[ -f firmware/vars.fd ] || cp firmware/OVMF_VARS.fd firmware/vars.fd
echo "Starting HydatekOS. Close the QEMU window, or use Shut Down in HydatekOS, to stop."
# HYDATEK_QEMU_EXTRA adds options, e.g. "-display none" for a run without a window
exec qemu-system-x86_64 -name HydatekOS -machine q35 -m 1G \
  -drive if=pflash,format=raw,readonly=on,file=firmware/OVMF_CODE.fd \
  -drive if=pflash,format=raw,file=firmware/vars.fd \
  -drive format=raw,file=hydatekos.img \
  -device qemu-xhci -device usb-kbd -device usb-tablet \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
  -device virtio-rng-pci \
  -serial file:hydatekos-log.txt \
  $HYDATEK_QEMU_EXTRA
