# HydatekOS architecture

```
 UEFI firmware ── loads \EFI\BOOT\BOOTX64.EFI (the HydatekOS kernel)
        │
        ▼
 main.rs  boot: disable watchdog → claim heap → pick display mode → splash
        │       → connect drivers → mount disk → start network → start shell
        │       → crossfade to lock screen → 100 Hz event loop
        ▼
 ┌────────────────────────────── shell/ ──────────────────────────────┐
 │ desktop: menu bar · widgets · window manager · dock · launcher ·   │
 │          notifications · menus                                     │
 │ mobile:  home screen · app grid · dock · full-screen apps          │
 │ mirrors: mobile screens rendered offscreen at 390x844, scaled in   │
 └───────────────┬───────────────────────────────┬────────────────────┘
                 │ Ui (immediate-mode toolkit)   │ App trait
                 ▼                               ▼
     gfx · font · icons · theme           apps/* (Files, Notes, …)
                 │                               │
                 ▼                               ▼
          back buffer ──GOP Blt──▶ screen    sys::Sys (shared state):
                                              settings · clock · Vfs · calendar · Link
```

## Boot and platform (`main.rs`, `efi.rs`, `heap.rs`)

- `efi.rs` holds hand-written bindings for the handful of UEFI services HydatekOS uses:
  graphics output, text input (extended), simple and absolute pointer, loaded image,
  simple file system, the RTC and reset.
- `heap.rs` is HydatekOS's allocator. It's a first-fit, address-ordered free list with
  coalescing, fed by one large page allocation (up to 1 GB) taken at boot. Every
  `Vec`/`String` in the system comes from it.
- `shell/splash.rs` draws the boot splash (logo, progress bar, status line) straight
  to the screen while `main.rs` brings the system up; it then crossfades into the
  first frame of the session.
- `shell/lock.rs` is the lock screen. The PIN and password are stored as salted,
  iterated SHA-256 hashes in `/system/lock.txt`; five wrong tries lock input for
  30 s. Fingerprint sign-in is delegated to the paired phone over Phone Link
  (`unlock_req` → the phone's BiometricPrompt → `unlock`), because HydatekOS has no
  fingerprint-reader drivers. The lock keeps people out of the session; it doesn't
  encrypt files on the disk. Its on-screen keyboard (`shell/osk.rs`) is shared
  with the setup assistant.
- `shell/setup.rs` is the setup assistant. It runs when `/system/profile.txt` has
  no name: name, picture, sign-in and look. On a computer that already has a PIN
  or password it runs only after unlocking, and leaves the sign-in step out.
  `profile.rs` holds the profile and its file format, and `avatar.rs` renders
  profile pictures into a canvas that `Ui::avatar` shows as a circle. See
  [PROFILE.md](PROFILE.md).
- The main loop waits on a 10 ms periodic timer event, so the CPU idles between frames.
  Each tick it polls input, advances the shell, and redraws only when something changed.
  Moving the pointer only re-blits the two small rectangles under the old and new cursor.

## Graphics (`gfx.rs`, `font.rs`, `icons.rs`)

The UEFI target is soft-float, so the rasteriser is integer-only:

- **Rounded rectangles and circles** use cached 4x4-supersampled quarter-circle coverage
  masks, one per radius. **Shadows** use a 9-slice of a blurred mask cached per
  (radius, blur).
- **Text** uses glyph atlases pre-rasterised at build time (`tools/fontgen.py`) at every
  size and scale the UI needs, blended with subpixel pen positioning. Glyphs a font
  lacks (₦, ₹, ₵) come from DejaVu Sans, and the italic faces are the regular and
  semibold weights slanted at build time.
- **Icons** are vector line drawings on a 24-unit grid (polylines, arcs, ellipses and
  fills). They're rasterised by capsule-distance tests with 4x4 supersampling and
  cached per pixel size.
- **Scaling:** `blit_scaled` box-filters with a rounded-corner mask. It's used to show a
  mobile screen inside a desktop window.

## UI toolkit (`ui.rs`)

Immediate mode: every frame the shell and apps re-describe the screen in *logical
points*, and `Ui` multiplies by the display scale. Interactive regions register a
`Zone { rect, Action }`. Hit-testing walks zones from the top down, so the last drawn
wins. Hover state from the previous frame drives highlights. Apps get `Action::App(instance, code)`.

## Apps (`apps/`)

```rust
trait App {
    fn kind(&self) -> AppKind;
    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32);
    fn action(&mut self, code: u32, double: bool, sys: &mut Sys);
    fn key(&mut self, k: Key, ctrl: bool, sys: &mut Sys);
    fn scroll(&mut self, dy: i32); fn tick(&mut self, sys: &mut Sys);
    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)>;   // File/Edit/View/Go
    ...
}
```

Apps render into whatever rectangle they're given: a desktop window, the local mobile
shell, or the mirrored phone. Below 540 points wide they switch to **compact phone
layouts** (sidebars become chips or back-navigable lists). Apps talk to the system
through `Sys`, and to the shell through requests like `Req::Open`, `Req::OpenPath` and
`Req::Toast`.

## Documents (`doc.rs`, `zip.rs`, `apps/scripts.rs`)

Hyda Scripts, the Hyda Workspace word processor, keeps a document as paragraphs
(style, alignment, characters, one formatting byte per character). `doc.rs` does the
editing operations, reads and writes the native `.hyds` format (line-based UTF-8
with a CRC-32 trailer), and imports and exports Word (`.docx`: Office Open XML
written by hand, read with a small XML tokenizer), plain text and Markdown.
Saving always writes `.hyds`; other formats are only ever read or exported. `zip.rs` writes
stored zip archives and reads stored or deflated ones with its own inflate.
`apps/scripts.rs` lays paragraphs out on A4 pages, draws them, and handles the caret,
selection (the shell forwards pointer drags to apps via `App::drag`, and
`input::shift()` reports Shift), undo, and the clipboard (`Sys::clipboard`). More in
[HYDA_WORKSPACE.md](HYDA_WORKSPACE.md).

## Spreadsheets (`grid.rs`, `gridio.rs`, `apps/grids.rs`)

Hyda Grids keeps a sheet as a sparse map of cells (the typed input and a format).
`grid.rs` tokenizes and parses formulas into an expression tree and evaluates them
with a per-frame cache and a guard against circular references. The UEFI target has
no floating-point library, so rounding, square roots, logarithms and powers are
implemented there too. `gridio.rs` reads and writes `.hydg` (line-based UTF-8 with a
CRC-32 trailer) and imports and exports `.xlsx` (SpreadsheetML, sharing the zip and
XML code with Hyda Scripts) and CSV.

## Storage (`fs.rs`)

`Vfs` keeps the file tree in memory and writes every change through to `\HYDATEK\` on
the boot volume, using the UEFI Simple File System. It's read back in full at boot.

```
/home/{Documents,Pictures,Downloads,Shared}   user files (Files app)
/trash                                        the Bin
/system/settings.txt, /system/calendar.txt    settings and events
/system/profile.txt, /system/profile.png      your profile and its photo
/system/lock.txt                              PIN and password hashes
```

If the boot volume is read-only, HydatekOS runs as a live session.

## Input (`input.rs`, `ps2.rs`)

The keyboard goes through Simple Text Input Ex (which carries Ctrl state). Pointers come
from every firmware Simple/Absolute Pointer. When the firmware binds no mouse driver,
HydatekOS drives the i8042 auxiliary port itself (`ps2.rs`), including the IntelliMouse
wheel. It only consumes bytes flagged as AUX, so the firmware keyboard driver keeps
working.

## Networking (`net/`)

- `snp.rs` claims the firmware's network card (UEFI Simple Network Protocol) and
  moves raw Ethernet frames. Everything above that is HydatekOS code.
- `mod.rs` implements Ethernet, ARP (cache plus queued packets), IPv4 (no
  fragmentation), ICMP echo, UDP and a DHCP client with renewal.
- `tcp.rs` is a server-side TCP. It has passive open, cumulative ACKs,
  out-of-order reassembly, go-back-N retransmission with exponential backoff,
  flow control, zero-window probing and orderly close.
- `mdns.rs` answers `hydatek-xxxx.local` and advertises `_hydatek-link._tcp`.

The main loop wakes on the 10 ms tick, or as soon as the card signals a packet.

## Phone Link (`link.rs`, `hlp.rs`, `linksrv.rs`, `apps/phonelink.rs`)

`linksrv.rs` serves HTTP and WebSocket on port 7743. It runs the encrypted HLP
session, feeds the `Link` model, and turns the UI's queued commands (reply to a
text, dial, send a file) into messages. See [PHONE_LINK.md](PHONE_LINK.md).
