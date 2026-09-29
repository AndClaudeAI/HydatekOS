//! The debug log (visible with `qemu -serial stdio`): COM1 on PCs, and on
//! ARM64 machines the firmware's serial port (EFI Serial I/O protocol), since
//! they have no COM1.

use core::fmt::{self, Write};

#[cfg(target_arch = "x86_64")]
const PORT: u16 = 0x3f8;

#[cfg(target_arch = "x86_64")]
pub fn init() {
    use crate::arch::outb;
    unsafe {
        outb(PORT + 1, 0x00);
        outb(PORT + 3, 0x80);
        outb(PORT, 0x01); // 115200 baud
        outb(PORT + 1, 0x00);
        outb(PORT + 3, 0x03);
        outb(PORT + 2, 0xc7);
        outb(PORT + 4, 0x0b);
    }
}

#[cfg(target_arch = "aarch64")]
#[repr(C)]
struct SerialIo {
    revision: u32,
    reset: usize,
    set_attributes: usize,
    set_control: usize,
    get_control: usize,
    write: extern "efiapi" fn(*mut SerialIo, *mut usize, *const u8) -> crate::efi::Status,
}

#[cfg(target_arch = "aarch64")]
static PORT_IO: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[cfg(target_arch = "aarch64")]
pub fn init() {
    const SERIAL_IO_GUID: crate::efi::Guid = crate::efi::Guid(0xBB25CF6F, 0xF1D4, 0x11D2, [0x9A, 0x0C, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0xFD]);
    if let Some(p) = crate::efi::locate::<SerialIo>(&SERIAL_IO_GUID) {
        PORT_IO.store(p as usize, core::sync::atomic::Ordering::Relaxed);
    }
}

pub struct Serial;

impl Write for Serial {
    #[cfg(target_arch = "x86_64")]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use crate::arch::{inb, outb};
        for b in s.bytes() {
            unsafe {
                let mut spins = 0;
                while inb(PORT + 5) & 0x20 == 0 && spins < 10000 {
                    spins += 1;
                }
                outb(PORT, b);
            }
        }
        Ok(())
    }

    #[cfg(target_arch = "aarch64")]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let p = PORT_IO.load(core::sync::atomic::Ordering::Relaxed) as *mut SerialIo;
        if !p.is_null() {
            // the terminal wants CR LF
            for part in s.split_inclusive('\n') {
                let (text, nl) = match part.strip_suffix('\n') {
                    Some(t) => (t, true),
                    None => (part, false),
                };
                let mut n = text.len();
                unsafe { ((*p).write)(p, &mut n, text.as_ptr()) };
                if nl {
                    let mut n = 2;
                    unsafe { ((*p).write)(p, &mut n, b"\r\n".as_ptr()) };
                }
            }
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {{
        use core::fmt::Write;
        let _ = writeln!($crate::serial::Serial, $($t)*);
    }};
}
