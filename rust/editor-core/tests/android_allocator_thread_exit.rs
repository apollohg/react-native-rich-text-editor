#[cfg(target_os = "android")]
use editor_core as _;

#[cfg(target_os = "android")]
const ZEROED_ALLOCATION_SIZES: &[usize] = &[1, 16, 64, 1024, 16384];
#[cfg(target_os = "android")]
const WORKER_THREAD_COUNT: usize = 64;

#[cfg(target_os = "android")]
fn allocate_zeroed_buffers(phase: &str) {
    for &size in ZEROED_ALLOCATION_SIZES {
        eprintln!(
            "[{phase}] alloc_zeroed({size}) on thread {:?}",
            std::thread::current().id()
        );
        let layout = std::alloc::Layout::from_size_align(size, 1).unwrap();
        let pointer = unsafe { std::alloc::alloc_zeroed(layout) };
        assert!(
            !pointer.is_null(),
            "[{phase}] alloc_zeroed({size}) returned null"
        );
        let bytes = unsafe { std::slice::from_raw_parts(pointer, size) };
        assert!(
            bytes.iter().all(|byte| *byte == 0),
            "[{phase}] alloc_zeroed({size}) returned dirty memory"
        );
        unsafe { std::alloc::dealloc(pointer, layout) };
    }
}

#[cfg(target_os = "android")]
fn release_allocator_thread_state_as_thread_exit_does() {
    unsafe { libmimalloc_sys::mi_thread_done() };
}

#[cfg(target_os = "android")]
fn main() {
    allocate_zeroed_buffers("process init thread before exit");
    release_allocator_thread_state_as_thread_exit_does();
    eprintln!("released allocator state of the thread that ran process init");
    allocate_zeroed_buffers("recycled thread pointer of exited process init thread");
    let workers: Vec<_> = (0..WORKER_THREAD_COUNT)
        .map(|index| {
            std::thread::spawn(move || allocate_zeroed_buffers(&format!("worker {index}")))
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker allocation panicked");
    }
    eprintln!("allocator survived the process init thread exiting");
}

#[cfg(not(target_os = "android"))]
fn main() {}
