#!/usr/bin/env bash
# Boot HydatekOS in QEMU with OVMF (UEFI) firmware.
#   tools/run-qemu.sh            build image and boot it in a window
#   HEADLESS=1 tools/run-qemu.sh  boot without a window (VNC :1, monitor on stdio)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/tools/mkimage.sh"
if [ -z "${OVMF_CODE:-}" ]; then
  for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/x64/OVMF_CODE.4m.fd \
           /usr/share/edk2-ovmf/x64/OVMF_CODE.fd /opt/homebrew/share/qemu/edk2-x86_64-code.fd /usr/local/share/qemu/edk2-x86_64-code.fd; do
    [ -f "$f" ] && { OVMF_CODE=$f; break; }
  done
fi
[ -n "${OVMF_CODE:-}" ] || { echo "OVMF firmware not found; install ovmf or set OVMF_CODE=/path/to/OVMF_CODE.fd"; exit 1; }
VARS="$ROOT/build/OVMF_VARS.fd"
if [ ! -f "$VARS" ]; then
  SRC_VARS="$(dirname "$OVMF_CODE")/$(basename "$OVMF_CODE" | sed 's/CODE/VARS/; s/-code/-vars/')"
  [ -f "$SRC_VARS" ] || SRC_VARS="$(dirname "$OVMF_CODE")/edk2-i386-vars.fd"   # Homebrew layout
  if [ -f "$SRC_VARS" ]; then cp "$SRC_VARS" "$VARS"; else truncate -s 540672 "$VARS"; fi
fi
ARGS=(
  -machine q35 -m 1G
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE"
  -drive if=pflash,format=raw,file="$VARS"
  -drive format=raw,file="$ROOT/build/hydatekos.img"
  -device qemu-xhci -device usb-kbd   # pointer: PS/2 mouse, driven by HydatekOS
  -serial file:"$ROOT/build/serial.log"
  -net none
)
if [ "${HEADLESS:-0}" = 1 ]; then
  exec qemu-system-x86_64 "${ARGS[@]}" -display none -vnc :1 -monitor "${MONITOR:-stdio}"
else
  exec qemu-system-x86_64 "${ARGS[@]}"
fi
