# HydatekOS roadmap

## M1 — "Dune" (this release)
- UEFI boot on x86-64; HydatekOS heap, compositor, window manager, fonts and icons
- Desktop shell and mobile shell from the design mockups
- Apps: Files, Notes, Settings, Calendar, Terminal, Phone Link, Messages, Mail, Browser, Music, Phone, Camera
- ✅ Hyda Workspace: Hyda Scripts, Hyda Grids and Hyda Slides, with Word, Excel and PowerPoint import and export
- ✅ First-start setup assistant and a profile (name, picture, sign-in, look), shown on the lock screen and desktop and recorded as the author of documents
- Persistent storage on the boot disk; light, dark and automatic themes; wallpapers (drawn scenes that follow the time of day, colours, your pictures) and an accent taken from the wallpaper
- PS/2 mouse driver; firmware keyboard, disk and framebuffer
- ✅ HydatekOS HID stack over the firmware's USB host: mice and tablets (ARM64 included), Precision Touchpads with gestures, touch screens, media keys, gamepads (HID, Xbox 360, Xbox One), haptic touchpad waveforms and controller rumble motors ([DRIVERS.md](DRIVERS.md))
- ✅ HID over I2C and a DesignWare I2C controller driver (tested against a simulated bus); Settings › Devices

## M2 — own the machine
- `ExitBootServices`; own page tables, GDT/IDT, APIC timer and interrupts
- Drivers: PS/2 keyboard, an xHCI host controller driver under the HID stack
- An ACPI AML interpreter, so I2C touchpads are found and started; Qualcomm GENI I2C and SPMI haptics (Snapdragon)
- AHCI and NVMe storage; a native FAT32 driver, then a HydatekOS file system
- Preemptive scheduler, processes and a syscall ABI; apps move out of the kernel
- ✅ JPEG, PNG, WebP, GIF and BMP decoders, SVG; pictures open from Files
- AVIF; a photo viewer with zoom and slideshows

## M3 — connected
- ✅ TCP/IP stack: ARP, IPv4, ICMP, UDP, DHCP, TCP, mDNS/DNS-SD (over the firmware NIC driver)
- ✅ Phone Link over the network: encrypted Hydatek Link Protocol, QR pairing,
  browser companion, HydatekOS Link for Android
- ✅ DNS resolver; TLS 1.3 / 1.2 with certificate checks
- ✅ Native Intel e1000 / e1000e / chipset LAN (82577 to I219) driver
- Native virtio-net and Realtek RTL8111 drivers; IPv6
- Wi-Fi (Intel iwlwifi-class hardware) with WPA2/WPA3
- ✅ Browser: HTML/CSS subset renderer, Hyda Search
- ✅ Browser pictures
- ✅ Browser: external stylesheets, media queries, CSS variables
- Browser: floats and positioning, tabs; Mail: IMAP/SMTP
- Phone Link: X25519 key exchange (HLP/2), real-phone screen mirroring, iOS companion app

## M4 — senses
- Intel HDA audio; Music playback
- Bluetooth (USB HCI), Phone Link calls over HFP
- USB video (UVC) cameras

## M5 — everyday OS
- ✅ Installer: Settings › Install puts HydatekOS on an empty internal disk (NVMe or SATA, 512-byte or 4K sectors) and adds it to the firmware's start-up menu
- Installing next to Windows or Linux (dual boot), resizing partitions
- ✅ Accounts: several people, each with their own files, settings and sign-in, a Shared folder, administrators
- Per-file permissions and sharing with one person, disk encryption, several people signed in at once
- Updates, Secure Boot signing, an app SDK and package format
- HydatekOS Mobile on ARM64 phones and tablets
