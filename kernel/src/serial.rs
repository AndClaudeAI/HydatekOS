//! COM1 debug log (visible with `qemu -serial stdio`).

use core::arch::asm;
use core::fmt::{self, Write};

const PORT: u16 = 0x3f8;

unsafe fn outb(port: u16, v: u8) {
    asm!("out dx, al", in("dx") port, in("al") v, options(nomem, nostack, preserves_flags));
}
unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags));
    v
}

pub fn init() {
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

pub struct Serial;

impl Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
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
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => {{
        use core::fmt::Write;
        let _ = writeln!($crate::serial::Serial, $($t)*);
    }};
}
