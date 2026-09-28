# HydatekOS roadmap

## M1 — "Dune" (this release)
- UEFI boot on x86-64; HydatekOS heap, compositor, window manager, fonts and icons
- Desktop shell and mobile shell from the design mockups
- Apps: Files, Notes, Settings, Calendar, Terminal, Phone Link, Messages, Mail, Browser, Music, Phone, Camera
- Persistent storage on the boot disk; light and dark themes
- PS/2 mouse driver; firmware keyboard, USB input, disk and framebuffer

## M2 — own the machine
- `ExitBootServices`; own page tables, GDT/IDT, APIC timer and interrupts
- Drivers: PS/2 keyboard, xHCI + USB HID (keyboard, mouse, touchpad), I2C-HID touchpads
- AHCI and NVMe storage; a native FAT32 driver, then a HydatekOS file system
- Preemptive scheduler, processes and a syscall ABI; apps move out of the kernel
- ✅ JPEG, PNG, WebP, GIF and BMP decoders, SVG; pictures open from Files
- AVIF; a photo viewer with zoom and slideshows

## M3 — connected
- ✅ TCP/IP stack: ARP, IPv4, ICMP, UDP, DHCP, TCP, mDNS/DNS-SD (over the firmware NIC driver)
- ✅ Phone Link over the network: encrypted Hydatek Link Protocol, QR pairing,
  browser companion, HydatekOS Link for Android
- ✅ DNS resolver; TLS 1.3 / 1.2 with certificate checks
- Native virtio-net, Intel e1000/i219 and Realtek RTL8111 drivers; IPv6
- Wi-Fi (Intel iwlwifi-class hardware) with WPA2/WPA3
- ✅ Browser: HTML/CSS subset renderer, Hyda Search
- ✅ Browser pictures
- Browser: external stylesheets, tabs; Mail: IMAP/SMTP
- Phone Link: X25519 key exchange (HLP/2), real-phone screen mirroring, iOS companion app

## M4 — senses
- Intel HDA audio; Music playback
- Bluetooth (USB HCI), Phone Link calls over HFP
- USB video (UVC) cameras

## M5 — everyday OS
- Graphical installer for internal disks with dual-boot support
- User accounts, permissions, disk encryption
- Updates, Secure Boot signing, an app SDK and package format
- HydatekOS Mobile on ARM64 phones and tablets
