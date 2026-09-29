# Drivers

HydatekOS's own device drivers, and what each piece of hardware needs.
Milestone 1 keeps the firmware for the USB host controller, disks and the
framebuffer (HydatekOS never calls `ExitBootServices`). Everything a person
touches (mice, touchpads, touch screens, media keys, game controllers,
haptics) now goes through HydatekOS code on both x86-64 and ARM64.

![Settings › Devices on ARM64](screenshots/arm64-devices.png)

## The pieces

| Driver | File | What it does |
|---|---|---|
| HID core | `kernel/src/hid.rs` | Reads HID report descriptors (globals, locals, push/pop, collections, arrays), finds what a device is, and decodes its reports: mice, tablets, Precision Touchpads, touch screens, gamepads, media keys and haptic controllers |
| Touchpad gestures | `kernel/src/touchpad.rs` | One finger moves; tap to click, two-finger tap for a right click, three for middle; two fingers scroll (natural direction); the pad's button, a right click with two fingers down; palms are ignored |
| USB HID | `kernel/src/usb.rs` | Takes every HID interface the firmware hasn't claimed, and Xbox controllers, through the firmware's USB I/O protocol. Asynchronous interrupt transfers fill a lock-free ring per device, and HydatekOS reads it every frame. New and unplugged devices are noticed within about 2 seconds |
| Game controllers | `kernel/src/gamepad.rs` | HID gamepads, Xbox 360 and Xbox One / Series protocols, navigation (D-pad or stick, A, B, Start, bumpers), rumble packets and motor timing |
| I²C and HID over I²C | `kernel/src/i2c.rs` | A polled DesignWare I²C controller driver (Intel and AMD laptops, many ARM SoCs) and Microsoft's HID over I²C protocol: HID descriptor, report descriptor, input reports, reset, power, get and set report, output reports |
| PS/2 mouse | `kernel/src/ps2.rs` | Older PCs and virtual machines without a USB mouse |
| Inventory | `kernel/src/hw.rs` | Every PCI device by class and vendor, and a count of the I²C devices ACPI lists; shown in Settings › Devices with who drives each |

## What works with what

| Hardware | x86-64 | ARM64 | How |
|---|---|---|---|
| USB mouse, trackball | ✅ | ✅ | HydatekOS USB HID (or the firmware's driver where it has one) |
| USB tablet, virtual machine pointer | ✅ | ✅ | positions scaled to the screen |
| USB Precision Touchpad | ✅ | ✅ | switched to multitouch mode (input mode 3), gestures |
| USB touch screen | ✅ | ✅ | points where it's touched; lifting the finger lets go |
| Media keys (mute, volume, brightness, sleep, eject) | ✅ | ✅ | the Consumer page, as the same keys as a keyboard's |
| USB haptic touchpad | ✅ | ✅ | reads its waveform list (GET_REPORT) and plays waveforms (SET_REPORT) |
| HID gamepad (DualShock 4, DualSense, generic) | ✅ | ✅ | D-pad and stick navigate, A Enter, B Escape, Start the start menu |
| Xbox 360 wired, Xbox One / Series (USB) | ✅ | ✅ | Microsoft's own protocols; the Xbox One is sent its "start" message |
| Rumble motors | ✅ | ✅ | Xbox 360, Xbox One, DualShock 4, DualSense |
| I²C touchpad, touch screen, pen | – | – | protocol ready; needs ACPI's AML to find the device |
| USB keyboard | ✅ | ✅ | the firmware's driver (HydatekOS reads it through the text input protocol) |
| USB storage | ✅ | ✅ | the firmware's driver |

## How a USB device is started

1. HydatekOS looks at every handle with the USB I/O protocol and skips those
   the firmware drives (they already have a keyboard or pointer protocol).
2. It reads the interface descriptor. HID interfaces (class 3) and Xbox pads
   (vendor class with Microsoft's subclasses 0x5D and 0x47) go on.
3. For HID it takes the report descriptor's length from the configuration
   descriptor's HID descriptor and reads it (GET_DESCRIPTOR 0x22). Then it
   switches boot devices to the report protocol (SET_PROTOCOL) and turns off
   idle repeats (SET_IDLE).
4. Touchpads are told to report fingers. Haptic touchpads are asked for their
   waveform list.
5. An asynchronous interrupt transfer starts on the interrupt IN endpoint.
   The firmware calls HydatekOS back with each report, into that device's ring.
6. The interrupt OUT endpoint, if there is one, carries rumble packets.
   Otherwise they go as SET_REPORT output reports.

The serial log says what happened. In QEMU's ARM machine that's
`usb: driving 0627:0001 interface 0 as Mouse`; an Xbox pad would log
`… as Xbox controller with rumble motors`.

## Tested

- **Host tests** (`cd tests-host && cargo test`):
  - descriptors from real devices: a boot mouse, QEMU's tablet, a two-finger
    Precision Touchpad, a haptic controller, media keys in both styles, a
    generic gamepad;
  - gesture sequences;
  - Xbox 360 and Xbox One reports;
  - navigation repeat timing;
  - every rumble packet, and motor timing;
  - malformed descriptors;
  - PCI class names;
  - finding I²C devices in AML;
  - the DesignWare driver and HID over I²C against a simulated controller, with a
    simulated haptic touchpad on its bus: descriptor, report descriptor, reset,
    power, waveform list, a haptic click, input mode, input reports, and report
    ids of 15 and up.
- **QEMU:**
  - ARM64 (virt, `qemu-xhci`, `usb-tablet`, `usb-mouse`): the pointer works,
    both absolute and relative, and clicks go through. This is the first time
    HydatekOS has a mouse on ARM.
  - x86-64 (q35): the USB tablet is driven by HydatekOS, alongside the PS/2 mouse.
- **Not yet on real hardware:** haptic touchpads, controllers and touch screens.

## What's next

- **ACPI AML interpreter.** It finds I²C touchpads: their controller, address
  and HID descriptor register (the PNP0C50 device's `_CRS` and `_DSM`), and the
  DesignWare controller's address and clock.
- **Qualcomm GENI I²C** for Snapdragon laptops' touchpads and touch screens.
  Then SPMI and the PMIC's haptics driver, for the vibration motor in
  Snapdragon tablets.
- **An xHCI host controller driver**, so USB no longer needs the firmware, and
  USB audio, video (UVC) and Bluetooth (HCI) can follow.
- **Wi-Fi, audio (HD Audio, SoundWire), NVMe/AHCI and GPU acceleration.** Each
  is a driver family of its own; see [ROADMAP.md](ROADMAP.md).
