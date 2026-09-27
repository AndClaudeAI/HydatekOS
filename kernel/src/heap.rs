//! Kernel heap: a first-fit, address-ordered free list with coalescing.
//!
//! One large region is claimed from the firmware at boot and managed here, so
//! every `Vec`/`String` in HydatekOS is served by our own allocator.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;

const HDR: usize = 16; // [block start, block length] stored before each allocation
const MIN_BLOCK: usize = 32;

struct FreeNode {
    size: usize,
    next: *mut FreeNode,
}

struct Inner {
    head: *mut FreeNode,
    total: usize,
    used: usize,
}

pub struct Heap(UnsafeCell<Inner>);
unsafe impl Sync for Heap {}

#[global_allocator]
pub static HEAP: Heap = Heap(UnsafeCell::new(Inner { head: null_mut(), total: 0, used: 0 }));

const fn align_up(x: usize, a: usize) -> usize {
    (x + a - 1) & !(a - 1)
}

impl Heap {
    /// Hand a region of memory to the allocator.
    pub unsafe fn add_region(&self, start: usize, len: usize) {
        let inner = &mut *self.0.get();
        let s = align_up(start, 16);
        let len = (len - (s - start)) & !15;
        inner.total += len;
        insert(inner, s, len);
    }

    pub fn stats(&self) -> (usize, usize) {
        let inner = unsafe { &*self.0.get() };
        (inner.used, inner.total)
    }
}

unsafe fn insert(inner: &mut Inner, start: usize, len: usize) {
    // Find insertion point in address order.
    let mut prev: *mut FreeNode = null_mut();
    let mut cur = inner.head;
    while !cur.is_null() && (cur as usize) < start {
        prev = cur;
        cur = (*cur).next;
    }
    let node = start as *mut FreeNode;
    (*node).size = len;
    (*node).next = cur;
    // Merge with the following block.
    if !cur.is_null() && start + len == cur as usize {
        (*node).size += (*cur).size;
        (*node).next = (*cur).next;
    }
    if prev.is_null() {
        inner.head = node;
    } else if prev as usize + (*prev).size == start {
        (*prev).size += (*node).size;
        (*prev).next = (*node).next;
    } else {
        (*prev).next = node;
    }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let inner = &mut *self.0.get();
        let align = layout.align().max(16);
        let size = align_up(layout.size().max(1), 16);
        let mut prev: *mut FreeNode = null_mut();
        let mut cur = inner.head;
        while !cur.is_null() {
            let start = cur as usize;
            let avail = (*cur).size;
            let user = align_up(start + HDR, align);
            let mut block = user + size - start;
            if block <= avail {
                let rest = avail - block;
                let next = (*cur).next;
                let replacement = if rest >= MIN_BLOCK {
                    let n = (start + block) as *mut FreeNode;
                    (*n).size = rest;
                    (*n).next = next;
                    n
                } else {
                    block = avail;
                    next
                };
                if prev.is_null() {
                    inner.head = replacement;
                } else {
                    (*prev).next = replacement;
                }
                let h = (user - HDR) as *mut usize;
                *h = start;
                *h.add(1) = block;
                inner.used += block;
                return user as *mut u8;
            }
            prev = cur;
            cur = (*cur).next;
        }
        null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let inner = &mut *self.0.get();
        let h = (ptr as usize - HDR) as *const usize;
        let start = *h;
        let len = *h.add(1);
        inner.used -= len;
        insert(inner, start, len);
    }
}
