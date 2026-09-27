#!/usr/bin/env bash
# Boot HydatekOS in QEMU with OVMF (UEFI) firmware.
#   tools/run-qemu.sh            build image and boot it in a window
#   HEADLESS=1 tools/run-qemu.sh  boot without a window (VNC :1, monitor on stdio)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/tools/mkimage.sh"
OVMF_CODE=${OVMF_CODE:-$(ls /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/x64/OVMF_CODE.4m.fd /opt/homebrew/share/qemu/edk2-x86_64-code.fd 2>/dev/null | head -1)}
VARS="$ROOT/build/OVMF_VARS.fd"
[ -f "$VARS" ] || cp "$(dirname "$OVMF_CODE")/$(basename "$OVMF_CODE" | sed 's/CODE/VARS/')" "$VARS" 2>/dev/null || truncate -s 528K "$VARS"
ARGS=(
  -machine q35 -m 1G
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE"
  -drive if=pflash,format=raw,file="$VARS"
  -drive format=raw,file="$ROOT/build/hydatekos.img"
  -device qemu-xhci -device usb-tablet -device usb-kbd
  -serial file:"$ROOT/build/serial.log"
  -net none
)
if [ "${HEADLESS:-0}" = 1 ]; then
  exec qemu-system-x86_64 "${ARGS[@]}" -display none -vnc :1 -monitor stdio
else
  exec qemu-system-x86_64 "${ARGS[@]}"
fi
