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
// Every pointer stays with MiMalloc, including frees on another thread.
unsafe impl GlobalAlloc for AndroidAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        initialize();
        mimalloc::MiMalloc.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        initialize();
        mimalloc::MiMalloc.alloc_zeroed(layout)
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
    use std::alloc::{alloc_zeroed, dealloc, realloc, Layout};

    #[test]
    fn android_allocator_preserves_alignment_zeroing_reallocation_and_thread_transfer() {
        for alignment in [1, 8, 64, 4096, 16384, 65536] {
            let size = alignment + 37;
            let layout = Layout::from_size_align(size, alignment).unwrap();
            let pointer = unsafe { alloc_zeroed(layout) };
            assert!(
                !pointer.is_null(),
                "allocation: alignment={alignment}, size={size}"
            );
            assert_eq!(
                pointer as usize % alignment,
                0,
                "initial alignment={alignment}"
            );
            let bytes = unsafe { std::slice::from_raw_parts_mut(pointer, size) };
            assert!(
                bytes.iter().all(|byte| *byte == 0),
                "zeroing: alignment={alignment}"
            );
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = index as u8;
            }
            let address = pointer as usize;
            std::thread::spawn(move || {
                let grown_size = size * 3;
                let grown = unsafe { realloc(address as *mut u8, layout, grown_size) };
                assert!(!grown.is_null(), "reallocation: alignment={alignment}");
                assert_eq!(grown as usize % alignment, 0, "grown alignment={alignment}");
                let bytes = unsafe { std::slice::from_raw_parts(grown, size) };
                for (index, byte) in bytes.iter().enumerate() {
                    assert_eq!(
                        *byte, index as u8,
                        "preserved byte {index}, alignment={alignment}"
                    );
                }
                unsafe {
                    dealloc(
                        grown,
                        Layout::from_size_align(grown_size, alignment).unwrap(),
                    )
                };
            })
            .join()
            .unwrap();
        }
    }
}
