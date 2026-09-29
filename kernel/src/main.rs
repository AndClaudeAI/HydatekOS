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
mod anim;
mod apps;
mod arch;
mod brand;
mod hw;
mod efi;
mod font;
mod gamepad;
mod fs;
mod gfx;
mod grid;
mod gridio;
mod haptics;
mod hid;
mod heap;
mod hlp;
mod i2c;
mod icons;
mod image;
mod input;
mod crypto;
mod doc;
mod deck;
mod deckio;
mod pdf;
mod profile;
mod accounts;
mod keymap;
mod lineedit;
mod avatar;
mod link;
mod linksrv;
mod net;
mod ps2;
mod qr;
mod rng;
mod shell;
mod sys;
mod theme;
mod touchpad;
mod tls;
mod ui;
mod usb;
mod web;
mod zip;

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
            // Keep the firmware's (usually native) mode unless it is tiny, and
            // keep its orientation: portrait tablets and phones stay portrait.
            let portrait = ch > cw;
            if (cw as i64) * (ch as i64) < 1024 * 640 {
                let mut best: Option<(u32, i64)> = None;
                for m in 0..mode.max_mode {
                    let mut size = 0usize;
                    let mut inf: *const efi::GopModeInfo = core::ptr::null();
                    if ((*gop).query_mode)(gop, m, &mut size, &mut inf) != efi::SUCCESS {
                        continue;
                    }
                    let (w, h) = ((*inf).hres as i64, (*inf).vres as i64);
                    if (*inf).pixel_format > 2 || (h > w) != portrait || w.max(h) > 1920 || w.min(h) > 1200 {
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

    /// Cross-fade the whole screen from `from` to `to`.
    fn crossfade(&self, from: &Canvas, to: &Canvas, scratch: &mut Vec<u32>) {
        scratch.clear();
        scratch.resize(from.px.len(), 0);
        for step in 1..=10u32 {
            let a = step * 256 / 10;
            for (i, px) in scratch.iter_mut().enumerate() {
                *px = gfx::lerp(from.px[i], to.px[i], a);
            }
            unsafe {
                ((*self.gop).blt)(self.gop, scratch.as_ptr(), 2, 0, 0, 0, 0, from.w as usize, from.h as usize, from.w as usize * 4);
            }
            efi::stall_us(12_000);
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
    arch::calibrate();
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
    let full = Rect::new(0, 0, disp.w, disp.h);
    // After the PC maker's logo: black, then the Hydatek Systems wordmark
    // fades in (about a third of a second).
    let mut splash = shell::splash::Splash::new(disp.w, disp.h, scale);
    for i in 0..=12u32 {
        disp.present(splash.fade_in(i * 255 / 12), full);
        efi::stall_us(25_000);
    }
    disp.present(splash.step(8, "Starting"), full);
    let rng_source = rng::init();
    log!("rng: {}", rng_source);
    disp.present(splash.step(20, "Connecting devices"), full);
    efi::connect_all();
    disp.present(splash.step(45, "Loading your files"), full);
    let vfs = fs::Vfs::mount();
    log!("storage: persistent={}", vfs.persistent);
    let mut sys = sys::Sys::new(vfs, efi::now());
    sys.rng_source = rng_source;
    disp.present(splash.step(65, "Starting network"), full);
    let mut net = net::Net::up();
    let mut server = net.as_mut().map(linksrv::LinkServer::new);
    let mut fetcher = web::fetch::Fetcher::new(sys.trust_store());
    if let Some(n) = net.as_ref() {
        sys.link.desktop_name = n.hostname.clone();
    }
    sys.firmware = efi::firmware_vendor();
    sys.hw = hw::detect();
    sys.mem_total = efi::total_memory();
    disp.present(splash.step(85, "Preparing your desktop"), full);
    let (lw, lh) = (disp.w / scale, disp.h / scale);
    let mut sh = shell::Shell::new(sys, lw, lh, scale);
    let mut back = Canvas::new(disp.w, disp.h);
    let cursor = Cursor::new(scale);
    let mut scratch: Vec<u32> = vec![];
    let mut input = input::Input::new(disp.w, disp.h);
    log!("input: {} pointer device(s)", input.pointer_count());

    // 100 Hz periodic timer to pace the loop.
    let mut timer: efi::Event = null_mut();
    (efi::bs().create_event)(0x8000_0000, 8, 0, null_mut::<c_void>(), &mut timer);
    (efi::bs().set_timer)(timer, 1, 100_000);

    let mut ticks: u64 = 0;
    let mut events = Vec::new();
    let (mut cx, mut cy) = (input.x, input.y);
    // Fade from the splash into the first frame (the lock screen or desktop).
    disp.present(splash.step(100, "Ready"), full);
    sh.render(&mut back, 0);
    disp.crossfade(&splash.frame, &back, &mut scratch);
    drop(splash);
    let mut first = true;
    loop {
        // Wake on the 10 ms tick, or as soon as a network packet arrives.
        let mut idx = 0usize;
        let waits = [timer, net.as_ref().map(|n| n.wait_event()).unwrap_or(timer)];
        (efi::bs().wait_for_event)(if net.is_some() { 2 } else { 1 }, waits.as_ptr(), &mut idx);
        if idx == 1 {
            if let (Some(n), Some(srv)) = (net.as_mut(), server.as_mut()) {
                n.poll(ticks * 10);
                if srv.poll(n, &mut sh.sys) {
                    sh.dirty = true;
                }
                if fetcher.poll(n, &mut sh.sys.web, ticks * 10) {
                    sh.dirty = true;
                }
            }
            continue;
        }
        ticks += 1;
        events.clear();
        input.poll(sh.sys.pointer_speed, &mut events);
        for ev in events.drain(..) {
            sh.event(ev, input.x, input.y);
        }
        sh.tick(ticks);
        // haptic feedback on HydatekOS's own devices: haptic touchpads and
        // controllers' rumble motors
        for (h, pulses) in core::mem::take(&mut sh.sys.haptics.device) {
            input.usb.feel(h, &pulses, sh.sys.haptics.strength.percent(), arch::ms());
        }
        if sh.sys.usb_gen != input.usb.generation {
            sh.sys.usb_gen = input.usb.generation;
            sh.sys.usb = input.usb.info.clone();
            sh.sys.haptic_pads = input.usb.haptic_pads();
            sh.sys.motors = input.usb.motors();
            sh.dirty = true;
        }
        if net.is_none() && !sh.sys.web.queue.is_empty() {
            // no network adapter: every request fails at once
            for r in core::mem::take(&mut sh.sys.web.queue) {
                sh.sys.web.done.push((r.id, Err(alloc::string::String::from("There's no network. Connect an Ethernet cable and try again."))));
            }
            sh.dirty = true;
        }
        if let (Some(n), Some(srv)) = (net.as_mut(), server.as_mut()) {
            n.poll(ticks * 10);
            if srv.poll(n, &mut sh.sys) {
                sh.dirty = true;
            }
            if fetcher.poll(n, &mut sh.sys.web, ticks * 10) {
                sh.dirty = true;
            }
            if ticks % 50 == 0 {
                let ip = if n.configured() { Some(n.ip) } else { None };
                let s = &mut sh.sys.net;
                if s.ip != ip || s.link_up != n.link_up() || !s.present {
                    sh.dirty = true;
                }
                *s = sys::NetStatus {
                    present: true,
                    name: n.if_name.clone(),
                    ip,
                    gw: n.gw,
                    dns: n.dns,
                    link_up: n.link_up(),
                    host: n.hostname.clone(),
                    rx: n.rx_packets,
                    tx: n.tx_packets,
                };
            }
        }
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
