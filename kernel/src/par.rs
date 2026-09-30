//! Every processor core drawing: pixel work split across the cores.
//!
//! At start-up the other cores (APs) are started once, through the
//! firmware's MP Services, into a loop of HydatekOS's own that waits for
//! work (pausing between looks; on ARM they sleep in WFE until the main
//! core signals). Handing them a job is then a few memory writes: starting
//! them through the firmware for every frame cost tens of milliseconds.
//!
//! They only ever run plain loops over pixels handed to them: they never
//! allocate, log or call the firmware, so nothing they touch needs a lock.
//! The main core takes a share too, then waits for the rest.

use crate::efi::{self, Guid};
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const MP_GUID: Guid = Guid(0x3FDDA605, 0xA76E, 0x4F46, [0xAD, 0x29, 0x12, 0xF4, 0x53, 0x1B, 0x3D, 0x08]);

#[repr(C)]
struct Mp {
    get_number_of_processors: extern "efiapi" fn(*mut Mp, *mut usize, *mut usize) -> efi::Status,
    get_processor_info: usize,
    startup_all_aps: extern "efiapi" fn(*mut Mp, extern "efiapi" fn(*mut c_void), bool, efi::Event, usize, *mut c_void, *mut *mut usize) -> efi::Status,
}

/// The job being worked on: parts handed out in turn.
struct Pool {
    f: UnsafeCell<Option<&'static (dyn Fn(usize) + Sync)>>,
    parts: AtomicUsize,
    next: AtomicUsize,
    done: AtomicUsize,
    /// bumped for every new job
    generation: AtomicUsize,
    /// workers inside a job right now
    inside: AtomicUsize,
    workers: AtomicUsize,
    running: AtomicBool,
    /// the workers go back to the firmware
    quit: AtomicBool,
}

unsafe impl Sync for Pool {}

static POOL: Pool = Pool {
    f: UnsafeCell::new(None),
    parts: AtomicUsize::new(0),
    next: AtomicUsize::new(0),
    done: AtomicUsize::new(0),
    generation: AtomicUsize::new(0),
    inside: AtomicUsize::new(0),
    workers: AtomicUsize::new(0),
    running: AtomicBool::new(false),
    quit: AtomicBool::new(false),
};

#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
static MWAIT: AtomicBool = AtomicBool::new(false);

fn relax() {
    // x86: sleep until the job word is written (MONITOR/MWAIT), where the
    // processor has it; otherwise a polite spin
    #[cfg(target_arch = "x86_64")]
    {
        if MWAIT.load(Ordering::Relaxed) {
            let a = &POOL.generation as *const AtomicUsize as usize;
            unsafe {
                core::arch::asm!("monitor", in("rax") a, in("ecx") 0, in("edx") 0, options(nostack));
                if POOL.generation.load(Ordering::Acquire) == SEEN_HINT.load(Ordering::Relaxed) {
                    core::arch::asm!("mwait", in("eax") 0, in("ecx") 0, options(nostack));
                }
            }
        } else {
            for _ in 0..32 {
                core::hint::spin_loop();
            }
        }
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("wfe", options(nomem, nostack))
    };
}

fn wake() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb ish", "sev", options(nomem, nostack))
    };
}

fn work() {
    let parts = POOL.parts.load(Ordering::Acquire);
    loop {
        let i = POOL.next.fetch_add(1, Ordering::AcqRel);
        if i >= parts {
            break;
        }
        if let Some(f) = unsafe { *POOL.f.get() } {
            f(i);
        }
        POOL.done.fetch_add(1, Ordering::AcqRel);
    }
}

/// The generation the workers last saw (for MWAIT's check).
static SEEN_HINT: AtomicUsize = AtomicUsize::new(0);

/// Where the other cores live from start-up on.
extern "efiapi" fn worker(_: *mut c_void) {
    POOL.workers.fetch_add(1, Ordering::AcqRel);
    let mut seen = POOL.generation.load(Ordering::Acquire);
    loop {
        if POOL.quit.load(Ordering::Acquire) {
            POOL.workers.fetch_sub(1, Ordering::AcqRel);
            return;
        }
        let g = POOL.generation.load(Ordering::Acquire);
        if g == seen {
            relax();
            continue;
        }
        seen = g;
        SEEN_HINT.store(g, Ordering::Relaxed);
        POOL.inside.fetch_add(1, Ordering::AcqRel);
        work();
        POOL.inside.fetch_sub(1, Ordering::AcqRel);
    }
}

static COUNT: AtomicUsize = AtomicUsize::new(1);

/// Jobs smaller than this many pixels stay on the calling core.
pub static MIN_PIXELS: AtomicUsize = AtomicUsize::new(256 * 1024);

/// Why the cores are (or aren't) drawing, for the log and Settings.
pub static mut DECISION: &str = "one core";

static MP: AtomicUsize = AtomicUsize::new(0);
static EVENT: AtomicUsize = AtomicUsize::new(0);
static CORES: AtomicUsize = AtomicUsize::new(1);

/// Send the workers back to the firmware (drawing on one core).
pub fn stop() {
    if !POOL.running.swap(false, Ordering::AcqRel) {
        return;
    }
    POOL.quit.store(true, Ordering::Release);
    wake();
    efi::wait_until(500, || POOL.workers.load(Ordering::Acquire) == 0);
    // the firmware knows they're back once its event is signalled
    let ev = EVENT.load(Ordering::Acquire) as efi::Event;
    efi::wait_until(500, || (efi::bs().check_event)(ev) == efi::SUCCESS);
    COUNT.store(1, Ordering::Release);
}

/// Start the workers (again).
pub fn start() -> usize {
    let mp = MP.load(Ordering::Acquire) as *mut Mp;
    let want = CORES.load(Ordering::Acquire).saturating_sub(1);
    if mp.is_null() || want == 0 || POOL.running.load(Ordering::Acquire) {
        return count();
    }
    POOL.quit.store(false, Ordering::Release);
    let ev = EVENT.load(Ordering::Acquire) as efi::Event;
    if unsafe { ((*mp).startup_all_aps)(mp, worker, false, ev, 0, core::ptr::null_mut(), core::ptr::null_mut()) } != efi::SUCCESS {
        return 1;
    }
    efi::wait_until(500, || POOL.workers.load(Ordering::Acquire) >= want);
    let n = POOL.workers.load(Ordering::Acquire) + 1;
    POOL.running.store(n > 1, Ordering::Release);
    COUNT.store(n, Ordering::Release);
    n
}

/// The frame-time trial: frames drawn with the workers, then without,
/// and whichever was faster stays. Called once per frame with its time.
pub struct Trial {
    with: (u64, u32),
    without: (u64, u32),
    pub done: bool,
}

pub const TRIAL_FRAMES: u32 = 60;

impl Trial {
    pub fn new() -> Trial {
        Trial { with: (0, 0), without: (0, 0), done: count() == 1 }
    }

    pub fn frame(&mut self, us: u64) {
        if self.done {
            return;
        }
        if self.with.1 < TRIAL_FRAMES {
            self.with = (self.with.0 + us, self.with.1 + 1);
            if self.with.1 == TRIAL_FRAMES {
                stop();
            }
        } else if self.without.1 < TRIAL_FRAMES {
            self.without = (self.without.0 + us, self.without.1 + 1);
            if self.without.1 == TRIAL_FRAMES {
                self.done = true;
                let (a, b) = (self.with.0 / TRIAL_FRAMES as u64, self.without.0 / TRIAL_FRAMES as u64);
                if a * 100 < b * 95 {
                    let n = start();
                    unsafe { DECISION = "every core" };
                    log!("par: frames take {} us on {} cores, {} us on one: drawing on every core", a, n, b);
                } else {
                    unsafe { DECISION = "one core (faster here)" };
                    log!("par: frames take {} us on every core, {} us on one: drawing on one", a, b);
                }
            }
        }
    }
}


/// Start the other cores (once, at start-up). Without MP Services, or with
/// one core, everything runs on the main core.
pub fn init() -> usize {
    let Some(mp) = efi::locate::<Mp>(&MP_GUID) else { return 1 };
    let (mut total, mut enabled) = (0usize, 0usize);
    if unsafe { ((*mp).get_number_of_processors)(mp, &mut total, &mut enabled) } != efi::SUCCESS || enabled < 2 {
        return 1;
    }
    let mut ev: efi::Event = core::ptr::null_mut();
    if (efi::bs().create_event)(0, 0, 0, core::ptr::null_mut::<c_void>(), &mut ev) != efi::SUCCESS {
        return 1;
    }
    MP.store(mp as usize, Ordering::Release);
    EVENT.store(ev as usize, Ordering::Release);
    CORES.store(enabled.min(16), Ordering::Release);
    #[cfg(target_arch = "x86_64")]
    {
        // CPUID.1:ECX bit 3: MONITOR/MWAIT
        let ecx = core::arch::x86_64::__cpuid(1).ecx;
        MWAIT.store(ecx & 8 != 0, Ordering::Relaxed);
    }
    // non-blocking (an event is given): they stay in HydatekOS's loop
    if unsafe { ((*mp).startup_all_aps)(mp, worker, false, ev, 0, core::ptr::null_mut(), core::ptr::null_mut()) } != efi::SUCCESS {
        return 1;
    }
    // wait until they're all in the loop
    let want = enabled - 1;
    efi::wait_until(500, || POOL.workers.load(Ordering::Acquire) >= want);
    let n = POOL.workers.load(Ordering::Acquire) + 1;
    unsafe { DECISION = "every core (being measured)" };
    POOL.running.store(n > 1, Ordering::Release);
    COUNT.store(n, Ordering::Release);
    n
}

pub fn count() -> usize {
    COUNT.load(Ordering::Acquire)
}

/// Run `f(0..parts)` on every core. `f` must only touch memory its part
/// owns (and not allocate).
pub fn run(parts: usize, f: &(dyn Fn(usize) + Sync)) {
    if parts <= 1 || !POOL.running.load(Ordering::Acquire) {
        for i in 0..parts {
            f(i);
        }
        return;
    }
    // the job, then the signal (the workers only read `f` after `generation`)
    unsafe { *POOL.f.get() = Some(core::mem::transmute::<&(dyn Fn(usize) + Sync), &'static (dyn Fn(usize) + Sync)>(f)) };
    POOL.parts.store(parts, Ordering::Release);
    POOL.done.store(0, Ordering::Release);
    POOL.next.store(0, Ordering::Release);
    POOL.generation.fetch_add(1, Ordering::AcqRel);
    wake();
    work();
    while POOL.done.load(Ordering::Acquire) < parts || POOL.inside.load(Ordering::Acquire) != 0 {
        core::hint::spin_loop();
    }
    unsafe { *POOL.f.get() = None };
}

/// Split `rows` rows into parts of about equal work for every core.
pub fn chunks(rows: usize) -> (usize, usize) {
    let parts = (count() * 2).min(rows.max(1));
    (parts, rows.div_ceil(parts))
}

/// Run `f(first_row, rows)` over a `w`-wide pixel buffer's rows on every
/// core, each core on its own rows.
/// Small jobs (under a quarter of a megapixel) stay on this core: handing
/// them out costs more than it saves.
pub fn rows(buf: &mut [u32], w: usize, f: &(dyn Fn(usize, &mut [u32]) + Sync)) {
    let h = if w == 0 { 0 } else { buf.len() / w };
    if buf.len() < MIN_PIXELS.load(Ordering::Relaxed) {
        f(0, buf);
        return;
    }
    let (parts, per) = chunks(h);
    let base = buf.as_mut_ptr() as usize;
    run(parts, &|i| {
        let (y0, y1) = (i * per, ((i + 1) * per).min(h));
        if y0 < y1 {
            // each part owns rows y0..y1 alone
            let part = unsafe { core::slice::from_raw_parts_mut((base as *mut u32).add(y0 * w), (y1 - y0) * w) };
            f(y0, part);
        }
    });
}
