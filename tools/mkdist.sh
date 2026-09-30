#!/usr/bin/env bash
# Build a ready-to-run HydatekOS package: a fresh disk image, the UEFI
# firmware and launchers for Windows, Mac and Linux, zipped.
#   tools/mkdist.sh     -> dist/HydatekOS-0.1-x86_64.zip
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NAME=HydatekOS-0.1
OUT="$ROOT/dist/$NAME"
OVMF_DIR="${OVMF_DIR:-/usr/share/OVMF}"
"$ROOT/tools/mkimage.sh" >/dev/null
rm -rf "$OUT" && mkdir -p "$OUT/firmware"
cp "$ROOT/build/hydatekos.img" "$OUT/"
cp "$OVMF_DIR/OVMF_CODE_4M.fd" "$OUT/firmware/OVMF_CODE.fd"
cp "$OVMF_DIR/OVMF_VARS_4M.fd" "$OUT/firmware/OVMF_VARS.fd"
cp /usr/share/doc/ovmf/copyright "$OUT/firmware/OVMF-LICENCE.txt"
cp "$ROOT/tools/dist/README.txt" "$ROOT/tools/dist/Start HydatekOS.bat" "$ROOT/tools/dist/start-hydatekos.command" "$OUT/"
# Windows wants CRLF in .bat and .txt files
python3 - "$OUT" <<'PY'
import sys, pathlib
for f in ("Start HydatekOS.bat", "README.txt"):
    p = pathlib.Path(sys.argv[1]) / f
    p.write_bytes(p.read_bytes().replace(b"\r\n", b"\n").replace(b"\n", b"\r\n"))
PY
# zip, keeping the Mac/Linux launcher executable
python3 - "$ROOT/dist" "$NAME" <<'PY'
import sys, os, zipfile, stat
base, name = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(os.path.join(base, name + "-x86_64.zip"), "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for dirpath, _, files in os.walk(os.path.join(base, name)):
        for f in sorted(files):
            full = os.path.join(dirpath, f)
            info = zipfile.ZipInfo.from_file(full, os.path.relpath(full, base))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (0o100755 if f.endswith(".command") else 0o100644) << 16
            with open(full, "rb") as fh:
                z.writestr(info, fh.read())
PY
# and the same system as files to copy onto any FAT32 flash drive
USB="$ROOT/dist/$NAME-USB"
rm -rf "$USB" && mkdir -p "$USB/EFI/BOOT" "$USB/HYDATEK"
cp "$ROOT/kernel/target/x86_64-unknown-uefi/release/hydatek.efi" "$USB/EFI/BOOT/BOOTX64.EFI"
ARM_EFI="$ROOT/kernel/target/aarch64-unknown-uefi/release/hydatek.efi"
[ -f "$ARM_EFI" ] && cp "$ARM_EFI" "$USB/EFI/BOOT/BOOTAA64.EFI"
APK="$ROOT/companion/android/build/hydatek-link.apk"
[ -f "$APK" ] && mkdir -p "$USB/HYDATEK/apps" && cp "$APK" "$USB/HYDATEK/apps/"
cp "$ROOT/tools/dist/USB-README.txt" "$USB/README.txt"
python3 - "$USB/README.txt" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
p.write_bytes(p.read_bytes().replace(b"\r\n", b"\n").replace(b"\n", b"\r\n"))
PY
(cd "$ROOT/dist" && rm -f "$NAME-USB-files.zip" && python3 -c "
import os, zipfile, sys
with zipfile.ZipFile('$NAME-USB-files.zip', 'w', zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for d, _, fs in os.walk('$NAME-USB'):
        for f in sorted(fs):
            full = os.path.join(d, f)
            z.write(full, os.path.relpath(full, '$NAME-USB'))
")
ls -la "$ROOT/dist/$NAME-x86_64.zip" "$ROOT/dist/$NAME-USB-files.zip"
