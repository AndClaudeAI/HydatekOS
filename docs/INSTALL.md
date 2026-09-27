# Installing HydatekOS on a PC

HydatekOS 0.1 runs on 64-bit x86 PCs with **UEFI firmware** (almost every PC made
since ~2012). You need 512 MB of RAM or more.

## 1. Build the disk image

```sh
rustup target add x86_64-unknown-uefi
sudo apt install mtools            # macOS: brew install mtools
tools/mkimage.sh                   # -> build/hydatekos.img (128 MB, GPT + EFI System Partition)
```

`SIZE_MB=1024 tools/mkimage.sh` makes a larger image if you want more room for files.

## 2. Write it to a USB stick

**This erases the stick.**

- **Windows:** [balenaEtcher](https://etcher.balena.io/), or Rufus with "DD image" mode.
- **macOS:** balenaEtcher, or `sudo dd if=build/hydatekos.img of=/dev/rdiskN bs=4m`.
- **Linux:** `sudo dd if=build/hydatekos.img of=/dev/sdX bs=4M status=progress conv=fsync`.

## 3. Boot it

1. Plug the stick in and power on the PC while pressing its boot-menu key
   (often F12, F11, F10, Esc or F8).
2. Choose the USB stick (it may be listed as "UEFI: <stick name>").
3. If the PC refuses to boot the stick, open the firmware settings and **disable Secure
   Boot**. HydatekOS isn't signed yet.

HydatekOS boots into the desktop. Files, notes, settings and calendar events are saved in
`\HYDATEK\` on the stick, so it works as a portable install you can carry between PCs.

## 4. Optional: install to the internal disk

Milestone 1 doesn't have its own installer yet (that's milestone 5). You can still put
HydatekOS on the internal disk next to Windows or Linux by hand. Every UEFI PC already
has an EFI System Partition (ESP):

1. Copy `kernel/target/x86_64-unknown-uefi/release/hydatek.efi` to the ESP as
   `\EFI\HydatekOS\hydatek.efi`.
   - Windows (admin prompt): `mountvol S: /s`, then
     `mkdir S:\EFI\HydatekOS` and `copy hydatek.efi S:\EFI\HydatekOS\`.
   - Linux: the ESP is usually mounted at `/boot/efi`.
2. Add a boot entry:
   - Windows: `bcdedit /copy {bootmgr} /d "HydatekOS"`, then
     `bcdedit /set {NEW-ID} path \EFI\HydatekOS\hydatek.efi`.
   - Linux: `sudo efibootmgr -c -d /dev/nvme0n1 -p 1 -L HydatekOS -l '\EFI\HydatekOS\hydatek.efi'`.
3. Reboot and pick "HydatekOS" from the firmware boot menu.

HydatekOS then creates `\HYDATEK\` on the ESP for your files. It never touches other
partitions.

## Hardware notes for milestone 1

| Area | Status |
|---|---|
| Display | Uses the firmware framebuffer (GOP) at the panel's native resolution; 2x scaling at 2560x1440+ |
| Keyboard | USB and PS/2, through the firmware |
| Mouse / touchpad | USB through the firmware, or the HydatekOS PS/2 mouse driver (with wheel) |
| Storage | The boot disk's FAT partition |
| Wi-Fi, Ethernet, Bluetooth, audio, camera | Not yet (see ROADMAP.md) |

If the pointer doesn't move on your machine, your firmware has no mouse driver and the
touchpad isn't PS/2-compatible. The keyboard still works, and native USB/I2C-HID drivers
arrive in milestone 2.
