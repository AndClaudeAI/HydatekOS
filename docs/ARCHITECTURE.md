# HydatekOS architecture

```
 UEFI firmware ── loads \EFI\BOOT\BOOTX64.EFI (the HydatekOS kernel)
        │
        ▼
 main.rs  boot: disable watchdog → claim heap → pick display mode → mount disk
        │       → connect drivers → start shell → 100 Hz event loop
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
- The main loop waits on a 10 ms periodic timer event, so the CPU idles between frames.
  Each tick it polls input, advances the shell, and redraws only when something changed.
  Moving the pointer only re-blits the two small rectangles under the old and new cursor.

## Graphics (`gfx.rs`, `font.rs`, `icons.rs`)

The UEFI target is soft-float, so the rasteriser is integer-only:

- **Rounded rectangles and circles** use cached 4x4-supersampled quarter-circle coverage
  masks, one per radius. **Shadows** use a 9-slice of a blurred mask cached per
  (radius, blur).
- **Text** uses glyph atlases pre-rasterised at build time (`tools/fontgen.py`) at every
  size and scale the UI needs, blended with subpixel pen positioning.
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

## Storage (`fs.rs`)

`Vfs` keeps the file tree in memory and writes every change through to `\HYDATEK\` on
the boot volume, using the UEFI Simple File System. It's read back in full at boot.

```
/home/{Documents,Pictures,Downloads,Shared}   user files (Files app)
/trash                                        the Bin
/system/settings.txt, /system/calendar.txt    settings and events
```

If the boot volume is read-only, HydatekOS runs as a live session.

## Input (`input.rs`, `ps2.rs`)

The keyboard goes through Simple Text Input Ex (which carries Ctrl state). Pointers come
from every firmware Simple/Absolute Pointer. When the firmware binds no mouse driver,
HydatekOS drives the i8042 auxiliary port itself (`ps2.rs`), including the IntelliMouse
wheel. It only consumes bytes flagged as AUX, so the firmware keyboard driver keeps
working.

## Phone Link (`link.rs`, `apps/phonelink.rs`, `apps/messages.rs`)

See [PHONE_LINK.md](PHONE_LINK.md).
