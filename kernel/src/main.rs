//! HydatekOS kernel entry point.
//!
//! Boot flow: UEFI firmware loads `\EFI\BOOT\BOOTX64.EFI` (this binary) →
//! we claim a heap, pick a display mode, mount the HydatekOS disk, start the
//! shell and run the event loop forever.

#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
mod serial;
mod apps;
mod efi;
mod font;
mod fs;
mod gfx;
mod heap;
mod icons;
mod input;
mod link;
mod ps2;
mod shell;
mod sys;
mod theme;
mod ui;

use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::null_mut;
use gfx::{Canvas, Rect};
use shell::cursor::Cursor;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log!("HydatekOS panic: {}", info);
    loop {
        efi::stall_us(1_000_000);
    }
}

fn claim_heap() -> usize {
    for mb in [1024usize, 768, 512, 384, 256, 192, 128, 96, 64] {
        let pages = mb * 256;
        let mut addr = 0u64;
        // AllocateAnyPages, EfiLoaderData
        if (efi::bs().allocate_pages)(0, 2, pages, &mut addr) == efi::SUCCESS {
            unsafe { heap::HEAP.add_region(addr as usize, pages * 4096) };
            return mb;
        }
    }
    0
}

struct Display {
    gop: *mut efi::Gop,
    w: i32,
    h: i32,
}

impl Display {
    fn init() -> Option<Display> {
        let gop: *mut efi::Gop = efi::locate(&efi::GOP_GUID)?;
        unsafe {
            let mode = &*(*gop).mode;
            let info = &*mode.info;
            let (cw, ch) = (info.hres as i32, info.vres as i32);
            log!("display: current mode {} {}x{} of {}", mode.mode, cw, ch, mode.max_mode);
            // Keep the firmware's (usually native) mode unless it is tiny.
            if cw < 1024 || ch < 640 {
                let mut best: Option<(u32, i64)> = None;
                for m in 0..mode.max_mode {
                    let mut size = 0usize;
                    let mut inf: *const efi::GopModeInfo = core::ptr::null();
                    if ((*gop).query_mode)(gop, m, &mut size, &mut inf) != efi::SUCCESS {
                        continue;
                    }
                    let (w, h) = ((*inf).hres as i64, (*inf).vres as i64);
                    if (*inf).pixel_format > 2 || w > 1920 || h > 1200 {
                        continue;
                    }
                    if best.map(|b| w * h > b.1).unwrap_or(true) {
                        best = Some((m, w * h));
                    }
                }
                if let Some((m, _)) = best {
                    ((*gop).set_mode)(gop, m);
                }
            }
            let info = &*(*(*gop).mode).info;
            Some(Display { gop, w: info.hres as i32, h: info.vres as i32 })
        }
    }

    fn present(&self, c: &Canvas, r: Rect) {
        let r = r.intersect(&c.bounds());
        if r.is_empty() {
            return;
        }
        unsafe {
            ((*self.gop).blt)(self.gop, c.px.as_ptr(), 2, r.x as usize, r.y as usize, r.x as usize, r.y as usize, r.w as usize, r.h as usize, c.w as usize * 4);
        }
    }

    /// Blit `r` of the back buffer with the cursor composited on top.
    fn present_with_cursor(&self, c: &Canvas, cur: &Cursor, cx: i32, cy: i32, r: Rect, scratch: &mut Vec<u32>) {
        let r = r.intersect(&c.bounds());
        if r.is_empty() {
            return;
        }
        scratch.clear();
        scratch.resize((r.w * r.h) as usize, 0);
        for y in 0..r.h {
            let src = ((r.y + y) * c.w + r.x) as usize;
            scratch[(y * r.w) as usize..((y + 1) * r.w) as usize].copy_from_slice(&c.px[src..src + r.w as usize]);
        }
        for y in 0..cur.h {
            let sy = cy + y - r.y;
            if sy < 0 || sy >= r.h {
                continue;
            }
            for x in 0..cur.w {
                let sx = cx + x - r.x;
                if sx < 0 || sx >= r.w {
                    continue;
                }
                let (col, a) = cur.px[(y * cur.w + x) as usize];
                if a == 0 {
                    continue;
                }
                let d = &mut scratch[(sy * r.w + sx) as usize];
                let a = a as u32;
                let mix = |s: u32, dd: u32| (s * a + dd * (255 - a)) / 255;
                let (sr, sg, sb) = ((col >> 16) & 255, (col >> 8) & 255, col & 255);
                let (dr, dg, db) = ((*d >> 16) & 255, (*d >> 8) & 255, *d & 255);
                *d = (mix(sr, dr) << 16) | (mix(sg, dg) << 8) | mix(sb, db);
            }
        }
        unsafe {
            ((*self.gop).blt)(self.gop, scratch.as_ptr(), 2, 0, 0, r.x as usize, r.y as usize, r.w as usize, r.h as usize, r.w as usize * 4);
        }
    }
}

#[no_mangle]
pub extern "efiapi" fn efi_main(image: efi::Handle, st: *mut efi::SystemTable) -> efi::Status {
    unsafe { efi::init(image, st) };
    serial::init();
    log!("HydatekOS 0.1 \"Dune\" booting");
    // Disable the firmware's 5-minute watchdog.
    (efi::bs().set_watchdog_timer)(0, 0, 0, core::ptr::null());
    let heap_mb = claim_heap();
    log!("heap: {} MB", heap_mb);
    if heap_mb == 0 {
        return 1 << 63 | 9;
    }
    let Some(disp) = Display::init() else {
        log!("no graphics output");
        return efi::NOT_FOUND;
    };
    log!("display: {}x{}", disp.w, disp.h);
    font::init();

    // Logical points: desktops at ~1280+ wide, phones/tablets in portrait at ~400-540.
    let scale = if disp.h > disp.w { (disp.w / 400).max(1) } else if disp.w >= 2560 && disp.h >= 1440 { 2 } else { 1 };
    let vfs = fs::Vfs::mount();
    log!("storage: persistent={}", vfs.persistent);
    let mut sys = sys::Sys::new(vfs, efi::now());
    sys.firmware = efi::firmware_vendor();
    sys.mem_total = efi::total_memory();
    let (lw, lh) = (disp.w / scale, disp.h / scale);
    let mut sh = shell::Shell::new(sys, lw, lh, scale);
    let mut back = Canvas::new(disp.w, disp.h);
    let cursor = Cursor::new(scale);
    let mut scratch: Vec<u32> = vec![];
    efi::connect_all();
    let mut input = input::Input::new(disp.w, disp.h);
    log!("input: {} pointer device(s)", input.pointer_count());

    // 100 Hz periodic timer to pace the loop.
    let mut timer: efi::Event = null_mut();
    (efi::bs().create_event)(0x8000_0000, 8, 0, null_mut::<c_void>(), &mut timer);
    (efi::bs().set_timer)(timer, 1, 100_000);

    let mut ticks: u64 = 0;
    let mut events = Vec::new();
    let (mut cx, mut cy) = (input.x, input.y);
    let mut first = true;
    loop {
        let mut idx = 0usize;
        (efi::bs().wait_for_event)(1, &timer, &mut idx);
        ticks += 1;
        events.clear();
        input.poll(sh.sys.pointer_speed, &mut events);
        for ev in events.drain(..) {
            sh.event(ev, input.x, input.y);
        }
        sh.tick(ticks);
        if sh.dirty || first {
            sh.dirty = false;
            first = false;
            sh.render(&mut back, ticks);
            disp.present(&back, back.bounds());
            disp.present_with_cursor(&back, &cursor, input.x, input.y, Rect::new(input.x, input.y, cursor.w, cursor.h), &mut scratch);
            cx = input.x;
            cy = input.y;
        } else if cx != input.x || cy != input.y {
            let old = Rect::new(cx, cy, cursor.w, cursor.h);
            let new = Rect::new(input.x, input.y, cursor.w, cursor.h);
            disp.present_with_cursor(&back, &cursor, input.x, input.y, old.union(&new), &mut scratch);
            cx = input.x;
            cy = input.y;
        }
    }
}
