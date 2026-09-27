//! Cryptographically secure random numbers.
//!
//! Seeded from every source available — the CPU's RDRAND instruction, the firmware
//! RNG protocol and timestamp-counter jitter — hashed together with SHA-256,
//! then expanded with ChaCha20 (fast key erasure).

use crate::crypto::{chacha20_block, Sha256};
use crate::efi;
use core::arch::x86_64::{__cpuid, _rdtsc};
use core::cell::UnsafeCell;

struct State {
    key: [u8; 32],
    counter: u64,
    seeded: bool,
}

struct Cell(UnsafeCell<State>);
unsafe impl Sync for Cell {}
static RNG: Cell = Cell(UnsafeCell::new(State { key: [0; 32], counter: 0, seeded: false }));

fn rdrand() -> Option<u64> {
    let leaf1 = __cpuid(1);
    if leaf1.ecx & (1 << 30) == 0 {
        return None;
    }
    for _ in 0..10 {
        let v: u64;
        let ok: u8;
        unsafe { core::arch::asm!("rdrand {v}", "setc {ok}", v = out(reg) v, ok = out(reg_byte) ok) };
        if ok != 0 {
            return Some(v);
        }
    }
    None
}

#[repr(C)]
struct RngProtocol {
    get_info: usize,
    get_rng: extern "efiapi" fn(*mut RngProtocol, *const efi::Guid, usize, *mut u8) -> efi::Status,
}

/// Gather entropy. Returns a short description of the sources used.
pub fn init() -> &'static str {
    let mut h = Sha256::new();
    let mut hw = false;
    for _ in 0..16 {
        if let Some(v) = rdrand() {
            h.update(&v.to_le_bytes());
            hw = true;
        }
    }
    let mut fw = false;
    if let Some(p) = efi::locate::<RngProtocol>(&efi::RNG_GUID) {
        let mut buf = [0u8; 32];
        if unsafe { ((*p).get_rng)(p, core::ptr::null(), 32, buf.as_mut_ptr()) } == efi::SUCCESS {
            h.update(&buf);
            fw = true;
        }
    }
    // Timer jitter: the low bits of the TSC around firmware stalls vary.
    for i in 0..256u32 {
        let t = unsafe { _rdtsc() };
        h.update(&t.to_le_bytes());
        if i % 32 == 0 {
            efi::stall_us(7);
        }
    }
    let t = efi::now();
    h.update(&[t.second, t.minute, t.hour, t.day, t.month]);
    h.update(&t.nanosecond.to_le_bytes());
    let st = unsafe { &mut *RNG.0.get() };
    st.key = h.finish();
    st.seeded = true;
    match (hw, fw) {
        (true, true) => "RDRAND + firmware RNG + TSC",
        (true, false) => "RDRAND + TSC",
        (false, true) => "firmware RNG + TSC",
        _ => "TSC jitter only",
    }
}

/// Mix extra event timing into the pool (called on input and network events).
pub fn stir(extra: u64) {
    let st = unsafe { &mut *RNG.0.get() };
    let mut h = Sha256::new();
    h.update(&st.key);
    h.update(&extra.to_le_bytes());
    h.update(&unsafe { _rdtsc() }.to_le_bytes());
    st.key = h.finish();
}

pub fn fill(out: &mut [u8]) {
    let st = unsafe { &mut *RNG.0.get() };
    if !st.seeded {
        init();
    }
    let mut nonce = [0u8; 12];
    for chunk in out.chunks_mut(32) {
        st.counter += 1;
        nonce[..8].copy_from_slice(&st.counter.to_le_bytes());
        let block = chacha20_block(&st.key, 0, &nonce);
        // first half re-keys (forward secrecy), second half is output
        st.key.copy_from_slice(&block[..32]);
        chunk.copy_from_slice(&block[32..32 + chunk.len()]);
    }
}

pub fn u32() -> u32 {
    let mut b = [0u8; 4];
    fill(&mut b);
    u32::from_le_bytes(b)
}
