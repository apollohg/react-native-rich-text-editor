use std::alloc::{GlobalAlloc, Layout};
use std::sync::Once;

struct AndroidAllocator;
#[global_allocator]
static ALLOCATOR: AndroidAllocator = AndroidAllocator;
static INITIALIZE: Once = Once::new();
unsafe extern "C" {
    fn editor_allocator_configure();
}
fn initialize() {
    // The C setter writes only allocator options and must not allocate.
    INITIALIZE.call_once(|| unsafe { editor_allocator_configure() });
}
// MiMalloc guarantees word alignment even for smaller allocations.
// Every pointer stays with MiMalloc, including frees on another thread.
unsafe impl GlobalAlloc for AndroidAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        initialize();
        if layout.align() <= std::mem::align_of::<usize>() {
            libmimalloc_sys::mi_malloc(layout.size()).cast()
        } else {
            mimalloc::MiMalloc.alloc(layout)
        }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        initialize();
        if layout.align() <= std::mem::align_of::<usize>() {
            libmimalloc_sys::mi_zalloc(layout.size()).cast()
        } else {
            mimalloc::MiMalloc.alloc_zeroed(layout)
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        mimalloc::MiMalloc.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        mimalloc::MiMalloc.realloc(ptr, layout, size)
    }
}

#[cfg(test)]
mod tests {
    use std::alloc::{alloc, alloc_zeroed, dealloc, realloc, Layout};

    const ALIGNMENTS: &[usize] = &[1, 2, 4, 8, 16, 32, 64, 4096, 16384, 65536];
    const BIN_BOUNDARY_SIZES: &[usize] = &[
        1, 3, 7, 8, 15, 16, 17, 24, 48, 95, 96, 97, 191, 192, 193, 383, 384, 385,
    ];
    const ODD_SIZE_OFFSET: usize = 37;
    const GROWTH_FACTOR: usize = 3;
    const SHRINK_DIVISOR: usize = 2;

    #[test]
    fn android_allocator_preserves_alignment_zeroing_reallocation_and_thread_transfer() {
        let mut allocations = Vec::new();
        for &alignment in ALIGNMENTS {
            let mut sizes = BIN_BOUNDARY_SIZES.to_vec();
            sizes.extend([
                alignment.saturating_sub(1).max(1),
                alignment,
                alignment + 1,
                alignment + ODD_SIZE_OFFSET,
            ]);
            sizes.sort_unstable();
            sizes.dedup();
            for size in sizes {
                let layout = Layout::from_size_align(size, alignment).unwrap();
                for zeroed in [false, true] {
                    let pointer = unsafe {
                        if zeroed {
                            alloc_zeroed(layout)
                        } else {
                            alloc(layout)
                        }
                    };
                    assert!(
                        !pointer.is_null(),
                        "allocation: {layout:?}, zeroed={zeroed}"
                    );
                    assert_eq!(pointer as usize % alignment, 0, "initial: {layout:?}");
                    if zeroed {
                        let bytes = unsafe { std::slice::from_raw_parts(pointer, size) };
                        assert!(bytes.iter().all(|byte| *byte == 0), "zeroing: {layout:?}");
                    }
                    for index in 0..size {
                        unsafe { pointer.add(index).write(index as u8) };
                    }
                    allocations.push((pointer as usize, layout));
                }
            }
        }
        std::thread::spawn(move || {
            for (address, original) in allocations {
                let mut pointer = address as *mut u8;
                let mut layout = original;
                for size in [
                    original.size() * GROWTH_FACTOR,
                    (original.size() / SHRINK_DIVISOR).max(1),
                ] {
                    let resized = unsafe { realloc(pointer, layout, size) };
                    assert!(!resized.is_null(), "reallocation: {layout:?} -> {size}");
                    assert_eq!(
                        resized as usize % layout.align(),
                        0,
                        "resized: {layout:?} -> {size}"
                    );
                    let preserved = unsafe {
                        std::slice::from_raw_parts(resized, layout.size().min(size))
                    };
                    for (index, byte) in preserved.iter().enumerate() {
                        assert_eq!(
                            *byte, index as u8,
                            "preserved byte {index}: {layout:?} -> {size}"
                        );
                    }
                    for index in 0..size {
                        unsafe { resized.add(index).write(index as u8) };
                    }
                    pointer = resized;
                    layout = Layout::from_size_align(size, layout.align()).unwrap();
                }
                unsafe { dealloc(pointer, layout) };
            }
        })
        .join()
        .unwrap();
    }
}
