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
mod acpi;
mod aml;
mod ambient;
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
mod hidin;
mod hda;
mod heap;
mod hlp;
mod i2c;
mod i2cdev;
mod icons;
mod image;
mod input;
mod bt;
mod crypto;
mod disks;
mod ahci;
mod nvme;
mod storage;
mod doc;
mod deck;
mod deckio;
mod pci;
mod par;
mod pdf;
mod personal;
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
mod sound;
mod sys;
mod theme;
mod touchpad;
mod tls;
mod ui;
mod uefiwifi;
mod usb;
mod web;
mod wifi;
mod xhci;
mod zip;
mod mkdisk;
mod install;

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

/// Present on every core (measured: see docs/GRAPHICS.md).
const PAR_PRESENT: bool = true;

struct Display {
    gop: *mut efi::Gop,
    w: i32,
    h: i32,
    /// the framebuffer, when HydatekOS may write it directly: address,
    /// pixels per scan line, red and blue swapped (RGBX)
    fb: Option<(usize, usize, bool)>,
    /// what's on the screen now (to write only what changed)
    shown: core::cell::RefCell<Vec<u32>>,
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
            Some(Display::adopt(gop))
        }
    }

    /// Switch to the firmware mode of `w` × `h`, if it has one: the
    /// resolution chosen in Settings › Display.
    fn switch(self, w: u32, h: u32) -> Display {
        unsafe {
            let gop = self.gop;
            let max = (*(*gop).mode).max_mode;
            for m in 0..max {
                let mut size = 0usize;
                let mut inf: *const efi::GopModeInfo = core::ptr::null();
                if ((*gop).query_mode)(gop, m, &mut size, &mut inf) != efi::SUCCESS {
                    continue;
                }
                if (*inf).hres == w && (*inf).vres == h && (*inf).pixel_format <= 2 {
                    if ((*gop).set_mode)(gop, m) == efi::SUCCESS {
                        log!("display: switched to {}x{} (chosen in Settings)", w, h);
                        return Display::adopt(gop);
                    }
                    break;
                }
            }
            log!("display: no {}x{} mode here; staying at {}x{}", w, h, self.w, self.h);
            self
        }
    }

    /// The display as the firmware's current mode has it.
    fn adopt(gop: *mut efi::Gop) -> Display {
        unsafe {
            let m = &*(*gop).mode;
            let info = &*m.info;
            // BGRX (1) is HydatekOS's own pixel layout; RGBX (0) swaps two bytes
            let fb = (m.fb_base != 0 && info.pixel_format <= 1).then(|| (m.fb_base as usize, info.pixels_per_scanline as usize, info.pixel_format == 0));
            log!("display: {}", if fb.is_some() { "direct framebuffer, every core" } else { "firmware blits" });
            Display { gop, w: info.hres as i32, h: info.vres as i32, fb, shown: core::cell::RefCell::new(vec![0x0102_0304; (info.hres * info.vres) as usize]) }
        }
    }

    fn present(&self, c: &Canvas, r: Rect) {
        let r = r.intersect(&c.bounds());
        if r.is_empty() {
            return;
        }
        if let Some((fb, stride, swap)) = self.fb {
            // straight into the framebuffer, on every core, only the spans
            // that changed since the last frame
            let mut shown = self.shown.borrow_mut();
            let w = c.w as usize;
            if shown.len() != c.px.len() {
                shown.clear();
                shown.resize(c.px.len(), 0x0102_0304);
            }
            let (x0, x1) = (r.x as usize, r.r() as usize);
            let (ry, rb) = (r.y as usize, r.b() as usize);
            let src = c.px.as_ptr() as usize;
            let rows = &mut shown[ry * w..rb * w];
            let draw = |y0: usize, part: &mut [u32]| {
                let src = src as *const u32;
                for (k, row) in part.chunks_mut(w).enumerate() {
                    let y = ry + y0 + k;
                    let s = unsafe { core::slice::from_raw_parts(src.add(y * w), w) };
                    // most rows haven't changed: one wide comparison says so
                    if row[x0..x1] == s[x0..x1] {
                        continue;
                    }
                    let a = (x0..x1).find(|&x| row[x] != s[x]).unwrap_or(x0);
                    let b = (x0..x1).rev().find(|&x| row[x] != s[x]).unwrap_or(x1 - 1);
                    let dst = unsafe { core::slice::from_raw_parts_mut((fb + 4 * y * stride) as *mut u32, stride) };
                    if swap {
                        for x in a..=b {
                            let p = s[x];
                            dst[x] = (p & 0xFF00_FF00) | (p >> 16 & 0xFF) | (p & 0xFF) << 16;
                        }
                    } else {
                        dst[a..=b].copy_from_slice(&s[a..=b]);
                    }
                    row[a..=b].copy_from_slice(&s[a..=b]);
                }
            };
            if PAR_PRESENT {
                let min = par::MIN_PIXELS.swap(0, core::sync::atomic::Ordering::Relaxed);
                par::rows(rows, w, &draw);
                par::MIN_PIXELS.store(min, core::sync::atomic::Ordering::Relaxed);
            } else {
                draw(0, rows);
            }
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
        self.shown.borrow_mut().iter_mut().for_each(|p| *p = 0x0102_0304);
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
        if let Some((fb, stride, swap)) = self.fb {
            // the cursor's little rectangle: written directly, and remembered
            // as what's on the screen (so the next frame repaints under it)
            let mut shown = self.shown.borrow_mut();
            let w = c.w as usize;
            for y in 0..r.h as usize {
                let (sy, row) = (r.y as usize + y, &scratch[y * r.w as usize..(y + 1) * r.w as usize]);
                let dst = (fb + 4 * (sy * stride + r.x as usize)) as *mut u32;
                for (x, p) in row.iter().enumerate() {
                    let q = if swap { (p & 0xFF00_FF00) | (p >> 16 & 0xFF) | (p & 0xFF) << 16 } else { *p };
                    unsafe { core::ptr::write_volatile(dst.add(x), q) };
                }
                if shown.len() >= (sy + 1) * w {
                    shown[sy * w + r.x as usize..sy * w + r.x as usize + row.len()].copy_from_slice(row);
                }
            }
            return;
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
    let cores = par::init();
    log!("par: drawing on {} core{}", cores, if cores == 1 { "" } else { "s" });
    let Some(mut disp) = Display::init() else {
        log!("no graphics output");
        return efi::NOT_FOUND;
    };
    log!("display: {}x{}", disp.w, disp.h);
    font::init();

    // Logical points: desktops at ~1280+ wide, phones/tablets in portrait at ~400-540.
    let auto_scale = |d: &Display| if d.h > d.w { (d.w / 400).max(1) } else if d.w >= 2560 && d.h >= 1440 { 2 } else { 1 };
    let mut scale = auto_scale(&disp);
    let mut full = Rect::new(0, 0, disp.w, disp.h);
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
    // the resolution and size chosen in Settings › Display
    let screen = vfs.read_raw("/system/screen.txt").map(|d| sys::ScreenChoice::parse(&alloc::string::String::from_utf8_lossy(&d))).unwrap_or_default();
    if let Some((w, h)) = screen.mode {
        if (w as i32, h as i32) != (disp.w, disp.h) {
            disp = disp.switch(w, h);
            full = Rect::new(0, 0, disp.w, disp.h);
            splash = shell::splash::Splash::new(disp.w, disp.h, auto_scale(&disp));
        }
    }
    scale = screen.scale_for(disp.w, disp.h).unwrap_or(auto_scale(&disp));
    log!("display: {}x{} at {}x", disp.w, disp.h, scale);
    disp.present(splash.step(45, "Loading your files"), full);
    let mut sys = sys::Sys::new(vfs, efi::now());
    sys.rng_source = rng_source;
    sys.screen_choice = screen;
    disp.present(splash.step(65, "Starting network"), full);
    let mut net = net::Net::up();
    let mut server = net.as_mut().map(linksrv::LinkServer::new);
    let mut fetcher = web::fetch::Fetcher::new(sys.trust_store());
    if let Some(n) = net.as_ref() {
        sys.link.desktop_name = n.hostname.clone();
    }
    sys.firmware = efi::firmware_vendor();
    sys.hw = hw::detect();
    // the firmware's ACPI namespace: devices off PCI and USB (I2C touchpads,
    // the battery, light sensors…)
    disp.present(splash.step(80, "Finding devices"), full);
    let mut acpi = acpi::load();
    for (path, id, kind) in acpi::inventory(&mut acpi) {
        let name = alloc::format!("{} ({})", aml::leaf(&path), id);
        let driver = match kind {
            "Battery" | "Ambient light sensor" | "Embedded controller" => "HydatekOS ACPI",
            "HID over I2C device" | "I2C controller (DesignWare)" => "HydatekOS I2C",
            _ => "Firmware (ACPI)",
        };
        sys.acpi_devices.push((name, alloc::string::String::from(kind), alloc::string::String::from(driver)));
    }
    let batteries = acpi::devices_with(&mut acpi, "PNP0C0A");
    let lights = acpi::devices_with(&mut acpi, "ACPI0008");
    sys.battery = batteries.first().and_then(|b| acpi::battery(&mut acpi, b));
    let i2c = i2cdev::I2cInput::start(&mut acpi);
    for f in &i2c.found {
        sys.acpi_devices.retain(|d| !(d.1 == "HID over I2C device" && f.name.starts_with(d.0.split(' ').next().unwrap_or(""))));
        sys.acpi_devices.push((f.name.clone(), f.kind.clone(), alloc::string::String::from(f.status)));
    }
    sys.mem_total = efi::total_memory();
    disp.present(splash.step(85, "Preparing your desktop"), full);
    let (lw, lh) = (disp.w / scale, disp.h / scale);
    let mut sh = shell::Shell::new(sys, lw, lh, scale);
    let mut back = Canvas::new(disp.w, disp.h);
    let cursor = Cursor::new(scale);
    let mut scratch: Vec<u32> = vec![];
    let mut under: Vec<u32> = vec![];
    // Wi-Fi through the firmware, where it has a driver
    let mut fwifi = uefiwifi::FirmwareWifi::find();
    if let Some(w) = fwifi.as_mut() {
        w.scan();
        sh.sys.wifi_nets = Some(Vec::new());
    }
    // sound
    let mut audio = hda::start();
    sh.sys.audio = audio.as_ref().map(|a| alloc::format!("{} · {}", a.name, a.outputs.join(", ")));
    // disks HydatekOS drives itself (not the boot disk)
    let mut disks = disks::start_all();
    sh.sys.disks = disks.info.clone();
    // the disks HydatekOS could be installed on (only ones it drives itself)
    sh.sys.install_targets = install::candidates(&mut disks);
    let mut install_job: Option<install::Job> = None;
    // USB controllers HydatekOS drives itself (before the firmware's USB
    // devices are listed: taking a controller removes them)
    let xhcis = xhci::start_all();
    // the PCI list says who drives what now
    for p in sh.sys.hw.pci.iter_mut() {
        let k = p.kind;
        if disks.taken.contains(&p.at) {
            p.driver = if k.starts_with("NVMe") { "HydatekOS NVMe" } else { "HydatekOS AHCI" };
        }
        if xhcis.iter().any(|x| x.at == p.at) {
            p.driver = "HydatekOS xHCI";
        }
        if audio.as_ref().map_or(false, |a| a.at == p.at) {
            p.driver = "HydatekOS HD Audio";
        }
    }
    let mut input = input::Input::new(disp.w, disp.h);
    input.i2c = Some(i2c);
    input.xhci = xhcis;
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
    sh.sys.sound(sound::Sound::Startup);
    drop(splash);
    let mut first = true;
    // every core or one: decided by timing real frames
    let mut trial = par::Trial::new();
    loop {
        // Wake on the 10 ms tick, or as soon as a network packet arrives.
        let mut idx = 0usize;
        let waits = [timer, net.as_ref().map(|n| n.wait_event()).unwrap_or(timer)];
        (efi::bs().wait_for_event)(if net.is_some() { 2 } else { 1 }, waits.as_ptr(), &mut idx);
        // installing HydatekOS on a disk, when Settings asks
        if let Some((i, bring)) = sh.sys.install_request.take() {
            let st = match sh.sys.install_targets.get(i).cloned() {
                Some(c) if c.ready.is_ok() => match install::files(&sh.sys.fs, bring).and_then(|items| install::Job::new(&mut disks, c.which, &c.name, &items, efi::now())) {
                    Ok(j) => {
                        install_job = Some(j);
                        install::State::Running { phase: "Checking the disk", done: 0, total: 1 }
                    }
                    Err(e) => install::State::Failed(e),
                },
                _ => install::State::Failed(alloc::string::String::from("That disk can't take HydatekOS.")),
            };
            if let install::State::Failed(e) = &st {
                log!("install: {}", e);
            }
            sh.sys.install_state = st;
            sh.dirty = true;
        }
        if let Some(job) = install_job.as_mut() {
            // about 40 ms of work, then a frame to show how it's going
            let t0 = arch::us();
            let mut st = install::State::Idle;
            while arch::us() - t0 < 40_000 {
                st = job.step(&mut disks, 256);
                if !matches!(st, install::State::Running { .. }) {
                    break;
                }
            }
            if !matches!(st, install::State::Running { .. }) {
                install_job = None;
                // the disk isn't empty any more
                sh.sys.install_targets = install::candidates(&mut disks);
            }
            sh.sys.install_state = st;
            sh.dirty = true;
        }
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
            if let Some(i) = input.i2c.as_mut() {
                let (w, n, p) = haptics::waveform(h);
                i.play(w, sh.sys.haptics.strength.percent(), n, p);
            }
        }
        // sounds: what the shell asked for, and the ring kept ahead of the hardware
        if let Some(a) = audio.as_mut() {
            for snd in core::mem::take(&mut sh.sys.sounds) {
                a.play(snd);
            }
            a.pump(sound::gain(sh.sys.volume, sh.sys.muted));
        } else {
            sh.sys.sounds.clear();
        }
        // Wi-Fi scans (every half minute, or when asked) and Bluetooth
        if let Some(w) = fwifi.as_mut() {
            if sh.sys.wifi_scan || ticks % 3000 == 2999 {
                sh.sys.wifi_scan = false;
                w.scan();
            }
            if w.poll() {
                sh.sys.wifi_nets = Some(w.networks.iter().map(|n| (n.ssid.clone(), n.security.name(), n.quality)).collect());
                sh.dirty = true;
            }
        }
        if ticks % 50 == 3 {
            if let Some(b) = input.usb.bts.first_mut() {
                if core::mem::take(&mut sh.sys.bt_scan) {
                    b.rescan();
                }
                let a = &b.adapter;
                let summary = alloc::format!("{} · {} · {}", if a.name.is_empty() { "Bluetooth adapter" } else { a.name.as_str() }, a.address(), bt::version_name(a.version));
                let near: Vec<(alloc::string::String, &'static str, i8)> = a.nearby.iter().map(|n| (n.label(), n.kind.name(), n.rssi)).collect();
                if sh.sys.bt_adapter.as_deref() != Some(summary.as_str()) || sh.sys.bt_nearby != near {
                    sh.sys.bt_adapter = Some(summary);
                    sh.sys.bt_nearby = near;
                    sh.dirty = true;
                }
            }
        }
        // the battery every 30 s, an ACPI light sensor every second
        if ticks % 3000 == 1 {
            if let Some(b) = batteries.first() {
                sh.sys.battery = acpi::battery(&mut acpi, b);
            }
        }
        if ticks % 100 == 7 {
            if let Some(l) = lights.first().and_then(|l| acpi::light(&mut acpi, l)) {
                sh.sys.lux = Some(ambient::smooth(sh.sys.lux, l));
            }
        }
        let usb_gen = input.usb.generation.wrapping_add(input.xhci.iter().map(|x| x.generation).fold(0u32, |a, g| a.wrapping_add(g)));
        if sh.sys.usb_gen != usb_gen {
            sh.sys.usb_gen = usb_gen;
            sh.sys.usb = input.usb.info.clone();
            for x in &input.xhci {
                sh.sys.usb.extend(x.info.iter().cloned());
            }
            sh.sys.haptic_pads = input.usb.haptic_pads() + input.i2c.as_ref().map_or(0, |i| i.haptic_pads());
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
            let t0 = arch::us();
            sh.render(&mut back, ticks);
            let t1 = arch::us();
            // the pointer goes into the frame before it's shown, so the
            // screen never has a frame without it (no flicker as things animate)
            cursor.stamp(&mut back, input.x, input.y, &mut under);
            disp.present(&back, back.bounds());
            cursor.unstamp(&mut back, input.x, input.y, &under);
            let t2 = arch::us();
            // frame times (smoothed) for Settings › Display
            let (r0, p0) = sh.sys.frame_us;
            sh.sys.frame_us = ((r0 * 7 + (t1 - t0) as u32) / 8, (p0 * 7 + (t2 - t1) as u32) / 8);
            sh.sys.frames += 1;
            trial.frame(t2 - t0);
            if sh.sys.frames % 500 == 0 {
                log!("frame: render {} us, present {} us", sh.sys.frame_us.0, sh.sys.frame_us.1);
            }
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
