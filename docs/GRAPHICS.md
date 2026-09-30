# Graphics: drawing fast without a GPU driver

HydatekOS draws everything in software and shows it through the firmware's
framebuffer. Drivers for the GPUs' own engines (Intel, AMD, NVIDIA, Qualcomm
Adreno) are not included yet: each is a family of drivers of its own, with the
vendor's firmware. This page describes how HydatekOS makes software drawing
fast, and how much each step saves.

## What it does

- **Direct framebuffer, changed pixels only.** Where the firmware's display
  mode has a framebuffer HydatekOS may write (BGRX or RGBX pixels),
  HydatekOS writes to it directly. It keeps a copy of what's on the screen,
  compares each row with a single wide comparison, and writes only the span
  that changed. The mouse pointer goes through the same path, so nothing is
  left behind it. Display modes without a writable framebuffer (ARM's
  virtio-gpu, for one) use the firmware's blit, as before.
- **Every core.** At start-up the other processor cores are started once,
  through the firmware's MP Services, into HydatekOS's own loop. They idle
  there: with MONITOR/MWAIT on x86 where the processor has it, WFE on ARM, a
  polite spin otherwise. Handing them work is then a few memory writes.
  Starting them through the firmware for each frame cost about 47 ms under
  QEMU, which is why they're started once and kept.
- **What runs on every core:**
  - the present;
  - big pixel passes: the wallpaper copy, brightness dimming, the window
    fades and scaled blits.

  Small jobs (under a quarter of a megapixel) stay on one core, because
  handing them out would cost more than it saves. The workers only run plain
  pixel loops: they never allocate, log or call the firmware.
- **Measured, not assumed.** The first 60 frames are drawn with the other
  cores and the next 60 without. Whichever was faster stays. In a virtual
  machine with more virtual cores than the host can run at once, the waiting
  cores can slow the main one down; there HydatekOS draws on one core.
  Settings › Display shows the decision and the frame time.
- **SIMD.** The pixel loops are written so the compiler vectorises them: SSE2
  on x86-64 (always present) and NEON on ARM64.

## Measured (QEMU x86-64, 4 cores, 1280 × 800)

QEMU emulates the processor, so these times are many times slower than real
hardware. The ratios are what matter.

| Present path | Time per full frame |
|---|---|
| Firmware blit (before) | 2.4 ms |
| Direct, first attempt (per-pixel volatile writes, cores started per frame) | 50.5 ms |
| Direct, one core, wide compares + memcpy | 2.9 ms |
| Direct, every core (started once) | **1.0 ms** |

Drawing a frame took 25-28 ms, and depends mostly on the shell's own drawing
(text, shapes, shadows). That code runs on the main core: it shares state
and allocates, so it can't be split across cores yet.

## Next

- Tiled drawing of the shell, so the drawing itself can run on every core
  (it needs a thread-safe heap and per-tile caches).
- AVX2 and wider paths, chosen by CPUID at run time.
- Write-combining for framebuffers the firmware maps uncached (x86 PAT).
- GPU drivers: virtio-gpu first (virtual machines), then Intel's display
  engine.
