//! Minimal bump allocator (alloc never reclaimed - sufficient for the GUI's
//! small, one-shot String usage; the kernel is single-threaded).

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

const HEAP_SIZE: usize = 128 * 1024;

struct Bump;

static HEAP_START: AtomicUsize = AtomicUsize::new(0);
static HEAP_POS: AtomicUsize = AtomicUsize::new(0);

static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

unsafe impl Sync for Bump {}

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if HEAP_START.load(Ordering::Relaxed) == 0 {
            let base = core::ptr::addr_of_mut!(HEAP) as usize;
            HEAP_START.store(base, Ordering::Relaxed);
            HEAP_POS.store(base, Ordering::Relaxed);
        }
        let align = layout.align().max(1);
        let mut pos = HEAP_POS.load(Ordering::Relaxed);
        let aligned = (pos + align - 1) & !(align - 1);
        let end = HEAP_START.load(Ordering::Relaxed) + HEAP_SIZE;
        if aligned + layout.size() > end {
            return core::ptr::null_mut();
        }
        pos = aligned + layout.size();
        HEAP_POS.store(pos, Ordering::Relaxed);
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // bump allocator: memory is never reclaimed
    }
}

#[global_allocator]
static A: Bump = Bump;
