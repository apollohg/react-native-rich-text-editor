use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static THREAD_LIVE_BYTES: Cell<isize> = const { Cell::new(0) };
}

struct ThreadAllocationMeter;

#[global_allocator]
static THREAD_ALLOCATION_METER: ThreadAllocationMeter = ThreadAllocationMeter;

fn record(delta: isize) {
    let _ = THREAD_LIVE_BYTES.try_with(|live| live.set(live.get() + delta));
}

unsafe impl GlobalAlloc for ThreadAllocationMeter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size() as isize);
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size() as isize);
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(-(layout.size() as isize));
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size as isize - layout.size() as isize);
        System.realloc(ptr, layout, size)
    }
}

pub(crate) fn thread_live_bytes() -> isize {
    THREAD_LIVE_BYTES.with(Cell::get)
}
