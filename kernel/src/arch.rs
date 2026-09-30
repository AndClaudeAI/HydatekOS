//! What differs between the processors HydatekOS runs on:
//! - **x86-64**: Intel and AMD PCs;
//! - **ARM64** (AArch64): Qualcomm Snapdragon laptops, and other ARM machines
//!   with UEFI firmware.
//!
//! The rest of HydatekOS only calls these functions, so one source tree
//! builds `BOOTX64.EFI` and `BOOTAA64.EFI`.

/// The architecture's name as Settings shows it.
#[cfg(target_arch = "x86_64")]
pub const NAME: &str = "x86-64";
#[cfg(target_arch = "aarch64")]
pub const NAME: &str = "ARM64";

/// A fast counter that always goes up (timing jitter for the random-number
/// pool, and measuring frames): the time-stamp counter, or the ARM generic
/// timer's virtual count.
pub fn cycles() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let v: u64;
        core::arch::asm!("mrs {v}, cntvct_el0", v = out(reg) v, options(nomem, nostack, preserves_flags));
        v
    }
}

/// Cycles of `cycles()` per millisecond, measured at start-up (`calibrate`).
static PER_MS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static START: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Measure the cycle counter against the firmware's delay (ARM64 knows its
/// timer's frequency), so `ms` tells real time.
pub fn calibrate() {
    use core::sync::atomic::Ordering;
    #[cfg(target_arch = "aarch64")]
    let per_ms = {
        let f: u64;
        unsafe { core::arch::asm!("mrs {v}, cntfrq_el0", v = out(reg) f, options(nomem, nostack, preserves_flags)) };
        f / 1000
    };
    #[cfg(target_arch = "x86_64")]
    let per_ms = {
        let t0 = cycles();
        crate::efi::stall_us(20_000);
        (cycles() - t0) / 20
    };
    PER_MS.store(per_ms.max(1), Ordering::Relaxed);
    START.store(cycles(), Ordering::Relaxed);
}

/// Milliseconds since `calibrate`: animations run on this, so they take
/// the same time however long a frame takes to draw.
pub fn ms() -> u64 {
    use core::sync::atomic::Ordering;
    let per = PER_MS.load(Ordering::Relaxed);
    if per == 0 {
        return 0;
    }
    cycles().wrapping_sub(START.load(Ordering::Relaxed)) / per
}

/// Microseconds since `calibrate` (timing frames).
pub fn us() -> u64 {
    use core::sync::atomic::Ordering;
    let per = PER_MS.load(Ordering::Relaxed);
    if per < 1000 {
        return ms() * 1000;
    }
    cycles().wrapping_sub(START.load(Ordering::Relaxed)) / (per / 1000)
}

/// A random number from the processor's own generator, if it has one:
/// RDRAND on x86-64, RNDR (Armv8.5 FEAT_RNG) on ARM64.
pub fn hw_random() -> Option<u64> {
    if !has_hw_random() {
        return None;
    }
    for _ in 0..10 {
        #[cfg(target_arch = "x86_64")]
        {
            let v: u64;
            let ok: u8;
            unsafe { core::arch::asm!("rdrand {v}", "setc {ok}", v = out(reg) v, ok = out(reg_byte) ok) };
            if ok != 0 {
                return Some(v);
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            let v: u64;
            let fail: u64;
            // RNDR sets NZCV to 0b0100 (Z) when it has no number ready
            unsafe { core::arch::asm!("mrs {v}, s3_3_c2_c4_0", "cset {f}, eq", v = out(reg) v, f = out(reg) fail) };
            if fail == 0 {
                return Some(v);
            }
        }
    }
    None
}

/// The processor's random-number instruction, by name.
pub const HW_RANDOM: &str = if cfg!(target_arch = "x86_64") { "RDRAND" } else { "RNDR" };

fn has_hw_random() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        core::arch::x86_64::__cpuid(1).ecx & (1 << 30) != 0
    }
    #[cfg(target_arch = "aarch64")]
    {
        // ID_AA64ISAR0_EL1.RNDR, bits 63:60
        id_aa64isar0() >> 60 != 0
    }
}

#[cfg(target_arch = "aarch64")]
fn id_aa64isar0() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mrs {v}, id_aa64isar0_el1", v = out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

#[cfg(target_arch = "aarch64")]
fn id_aa64pfr0() -> u64 {
    let v: u64;
    unsafe { core::arch::asm!("mrs {v}, id_aa64pfr0_el1", v = out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// PC I/O ports (x86 only: the serial log and the PS/2 mouse). ARM machines
/// have no port space; reads give 0xFF (nothing there) and writes go nowhere.
pub unsafe fn inb(port: u16) -> u8 {
    #[cfg(target_arch = "x86_64")]
    {
        let v: u8;
        core::arch::asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags));
        v
    }
    #[cfg(target_arch = "aarch64")]
    {
        let _ = port;
        0xFF
    }
}

pub unsafe fn outb(port: u16, v: u8) {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags));
    #[cfg(target_arch = "aarch64")]
    let _ = (port, v);
}

pub unsafe fn inw(port: u16) -> u16 {
    #[cfg(target_arch = "x86_64")]
    {
        let v: u16;
        core::arch::asm!("in ax, dx", out("ax") v, in("dx") port, options(nomem, nostack, preserves_flags));
        v
    }
    #[cfg(target_arch = "aarch64")]
    {
        let _ = port;
        0xFFFF
    }
}

pub unsafe fn inl(port: u16) -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        let v: u32;
        core::arch::asm!("in eax, dx", out("eax") v, in("dx") port, options(nomem, nostack, preserves_flags));
        v
    }
    #[cfg(target_arch = "aarch64")]
    {
        let _ = port;
        0xFFFF_FFFF
    }
}

pub unsafe fn outw(port: u16, v: u16) {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!("out dx, ax", in("dx") port, in("ax") v, options(nomem, nostack, preserves_flags));
    #[cfg(target_arch = "aarch64")]
    let _ = (port, v);
}

pub unsafe fn outl(port: u16, v: u32) {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!("out dx, eax", in("dx") port, in("eax") v, options(nomem, nostack, preserves_flags));
    #[cfg(target_arch = "aarch64")]
    let _ = (port, v);
}

/// What the processor says about itself.
pub struct CpuId {
    /// "GenuineIntel", "AuthenticAMD"; on ARM the implementer's name
    pub vendor: alloc::string::String,
    /// the marketing name the processor reports (x86 brand string), or on ARM
    /// the core design from MIDR ("Qualcomm Oryon", "Arm Cortex-A72")
    pub model: alloc::string::String,
    /// instruction-set features HydatekOS knows and cares about
    pub features: alloc::vec::Vec<&'static str>,
    /// ARM: the Main ID Register (implementer, part), 0 on x86
    pub midr: u64,
}

pub fn cpu() -> CpuId {
    use alloc::string::String;
    use alloc::vec::Vec;
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::__cpuid;
        let l0 = __cpuid(0);
        let mut vendor = Vec::new();
        for r in [l0.ebx, l0.edx, l0.ecx] {
            vendor.extend_from_slice(&r.to_le_bytes());
        }
        let mut brand = Vec::new();
        if __cpuid(0x8000_0000).eax >= 0x8000_0004 {
            for leaf in 0x8000_0002u32..=0x8000_0004 {
                let r = __cpuid(leaf);
                for v in [r.eax, r.ebx, r.ecx, r.edx] {
                    brand.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        let l1 = __cpuid(1);
        let l7 = if l0.eax >= 7 { core::arch::x86_64::__cpuid_count(7, 0) } else { core::arch::x86_64::CpuidResult { eax: 0, ebx: 0, ecx: 0, edx: 0 } };
        let mut features = Vec::new();
        for (on, name) in [
            (l1.edx & (1 << 26) != 0, "SSE2"),
            (l1.ecx & (1 << 19) != 0, "SSE4.1"),
            (l1.ecx & (1 << 28) != 0, "AVX"),
            (l7.ebx & (1 << 5) != 0, "AVX2"),
            (l7.ebx & (1 << 16) != 0, "AVX-512"),
            (l1.ecx & (1 << 25) != 0, "AES-NI"),
            (l7.ebx & (1 << 29) != 0, "SHA"),
            (l1.ecx & (1 << 30) != 0, "RDRAND"),
        ] {
            if on {
                features.push(name);
            }
        }
        let text = |b: Vec<u8>| String::from(String::from_utf8_lossy(&b).trim_matches(|c: char| c == '\0' || c.is_whitespace()));
        CpuId { vendor: text(vendor), model: text(brand), features, midr: 0 }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let midr: u64;
        unsafe { core::arch::asm!("mrs {v}, midr_el1", v = out(reg) midr, options(nomem, nostack, preserves_flags)) };
        let (vendor, model) = crate::hw::arm_core(midr);
        let isar0 = id_aa64isar0();
        let pfr0 = id_aa64pfr0();
        let mut features = Vec::new();
        for (on, name) in [
            // AdvSIMD (NEON) is 0 when present, 0xF when not
            ((pfr0 >> 20) & 0xF != 0xF, "NEON"),
            ((pfr0 >> 32) & 0xF != 0, "SVE"),
            ((isar0 >> 4) & 0xF != 0, "AES"),
            ((isar0 >> 12) & 0xF != 0, "SHA2"),
            ((isar0 >> 20) & 0xF != 0, "Atomics"),
            (isar0 >> 60 != 0, "RNDR"),
        ] {
            if on {
                features.push(name);
            }
        }
        CpuId { vendor: String::from(vendor), model, features, midr }
    }
}
