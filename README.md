# HydatekOS

A calm desktop **and** mobile operating system, written from scratch.

HydatekOS boots straight from a PC's UEFI firmware. The kernel, memory allocator,
graphics compositor, window manager, font and icon renderers, file system layer,
PS/2 mouse driver and every app are HydatekOS code: Rust, `no_std`, zero
third-party crates. It isn't a skin on Windows, macOS or Linux.

| Desktop | Phone Link (live phone mirror) | Mobile shell |
|---|---|---|
| ![Desktop](docs/screenshots/desktop.png) | ![Phone Link](docs/screenshots/phone-link.png) | ![Mobile shell](docs/screenshots/mobile-shell.png) |

## What works today (milestone 1, "Dune")

- **Boots on real x86-64 PCs** from a USB stick or the internal disk (UEFI, Secure Boot off).
- **Desktop shell:** menu bar with working menus, clock and "Up next" widget, quick
  toggles, a dock with running-app indicators and tooltips, an app launcher with search,
  and notifications.
- **Window manager:** overlapping windows with drag, resize, maximise (double-click the
  header), minimise, close, focus and soft shadows.
- **Mobile shell:** the phone home screen from the design (clock, Up next card, app grid,
  dock) with full-screen apps and a home indicator. It's used automatically on portrait
  screens and can be switched on from **View › Mobile Shell** or Settings.
- **Phone Link** (like Windows "Link to Windows"): pair a phone and get its messages
  (read and reply), notifications, photos (copy to Pictures), call log and a **live,
  interactive mirror of the phone's screen**.
- **Apps:** Files, Notes, Settings, Calendar, Terminal (`hsh`), Phone Link, Messages,
  Mail, Browser, Music, plus Phone and Camera on mobile.
- **Persistent storage:** your files, settings and calendar live in `\HYDATEK\` on the
  boot disk and survive reboots.
- **Light "Dune" and dark "Dusk" themes** with four accent colours.

### What isn't there yet

An operating system is a long project. Here's what's still missing; the plan is in
[docs/ROADMAP.md](docs/ROADMAP.md).

- **No network stack or Wi-Fi/Bluetooth drivers yet.** Browser shows built-in
  `hydatek://` pages, and Mail holds local messages.
- **Phone Link runs against a virtual HydatekOS Mobile device** that lives inside the
  OS. The desktop side and the protocol ([docs/PHONE_LINK.md](docs/PHONE_LINK.md)) are
  in place. Pairing a real handset needs the network drivers above and a companion
  phone app.
- **No sound or camera drivers yet.**
- Milestone 1 still uses the firmware for USB input, disk access and the framebuffer
  (it never calls `ExitBootServices`). Milestone 2 replaces these with HydatekOS drivers.

## Try it in a virtual machine

Prerequisites: Rust (stable) with the UEFI target, `mtools`, `python3`, QEMU and OVMF.

```sh
rustup target add x86_64-unknown-uefi
sudo apt install qemu-system-x86 ovmf mtools     # macOS: brew install qemu mtools
tools/run-qemu.sh
```

Click inside the QEMU window to capture the mouse (Ctrl+Alt+G releases it).

## Install on a PC

See **[docs/INSTALL.md](docs/INSTALL.md)**. In short:

```sh
tools/mkimage.sh        # -> build/hydatekos.img
```

Then write `build/hydatekos.img` to a USB stick with balenaEtcher, Rufus (DD mode) or
`dd`, and boot from it. Your files are saved on the stick. The install guide also
covers putting HydatekOS on the internal disk next to another OS.

## Using HydatekOS

| Action | How |
|---|---|
| Open any app | Grid button in the dock, the search icon in the menu bar, or **F1** |
| Move / maximise a window | Drag the header / double-click it |
| Resize a window | Drag the bottom-right corner |
| Close the front window | **Ctrl+W** or the orange button |
| Save a note | **Ctrl+S** |
| Rename / delete a file | **F2** / **Delete** (File menu has the same actions) |
| Shell commands | Open Terminal and type `help` |
| Restart / shut down | HydatekOS logo menu, or Settings › About |

## Repository layout

```
kernel/            the HydatekOS kernel + shell (Rust, no_std, UEFI x86-64)
  src/main.rs      boot, heap, display, main loop, cursor
  src/efi.rs       hand-written UEFI bindings
  src/heap.rs      kernel memory allocator
  src/gfx.rs       software rasteriser (AA shapes, shadows, scaling)
  src/font.rs      text rendering from the prebuilt font pack
  src/icons.rs     vector icon set
  src/ps2.rs       PS/2 mouse driver
  src/input.rs     keyboard + pointer input
  src/fs.rs        virtual file system persisted to the boot disk
  src/shell/       desktop shell, window manager, mobile shell, wallpaper
  src/apps/        built-in applications
  assets/fonts.bin prebuilt glyph atlases (regenerate with tools/fontgen.py)
assets/fonts/      source fonts (Figtree, Bodoni Moda, DejaVu Sans Mono) + licences
tools/             image builder, QEMU runner, GPT writer, font generator
docs/              architecture, install guide, Phone Link protocol, roadmap
```

More detail: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Licences

The fonts are distributed under their own licences in `assets/fonts/` (SIL OFL 1.1 for
Figtree and Bodoni Moda, the Bitstream Vera/DejaVu licence for DejaVu Sans Mono).
