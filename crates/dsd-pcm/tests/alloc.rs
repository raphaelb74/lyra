//! The conversion loop allocates nothing once its buffers fit the chunk
//! size. Its own test binary: the counting allocator is process-wide.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use dsd_pcm::Converter;

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged; the only
// addition is an atomic counter, which does not allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System.alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn steady_state_conversion_does_not_allocate() {
    for dsd_rate in [2_822_400, 11_289_600] {
        let mut converter = Converter::new(dsd_rate, 88_200, 2, -3.0).unwrap();
        // A pseudo-random stream: every table entry and path gets used.
        let chunk: Vec<u8> = (0..32_768u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13).to_le_bytes()[0])
            .collect();
        let mut out = Vec::new();
        for _ in 0..3 {
            out.clear();
            converter.push(&chunk, &mut out);
        }
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        for _ in 0..100 {
            out.clear();
            converter.push(&chunk, &mut out);
        }
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert_eq!(after - before, 0, "allocations at DSD rate {dsd_rate}");
    }
}
