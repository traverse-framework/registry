//! Shared `no_std` runtime shim for Traverse capability agents.
//!
//! Exists so each capability crate under `agents/` only has to write its own
//! pure input->output logic, not re-derive the WASI plumbing. Verified
//! ABI-compliant: the only imports this produces are
//! `wasi_snapshot_preview1::{fd_read, fd_write, proc_exit}` -- the exact
//! whitelist Traverse's `WasmExecutor` enforces (see
//! docs/decision-log.md for the finding that motivated this crate: an
//! earlier `std` + `serde_json` + `wasm32-wasip1` build imported
//! `environ_get`/`environ_sizes_get`, which that whitelist rejects).
//!
//! `#[global_allocator]` and `#[panic_handler]` are only defined for real
//! `wasm32` builds (`cfg(not(test))`) so `cargo test` on the host still runs
//! against ordinary `std`, without a duplicate-lang-item conflict.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod json;
pub use json::{array_of_strings, object, parse as parse_json, write as write_json, Value};

#[cfg(all(target_arch = "wasm32", not(test)))]
mod allocator {
    use core::alloc::{GlobalAlloc, Layout};
    use core::cell::UnsafeCell;

    // 192 MiB. Was 1 MiB ("generous for one small JSON request/response")
    // until the catalog-builder build-tool capability (registry#105) needed
    // to hold the entire capabilities/ tree's contracts (input parse tree +
    // a full-contract-cloning output tree + the final serialized JSON, none
    // of it ever freed by this bump allocator) in memory at once -- a
    // fundamentally different workload from the small, fixed-shape
    // request/response every other capability here processes. Raised 16 -> 64
    // MiB (registry#384): the per-capability spec-024 risk projection (`risk`
    // object + `is_automatic_eligible` + `risk_source`) is carried on every
    // one of the ~136 entries in both the input tree and the cloned output
    // tree, which tipped the 16 MiB peak over into a bump-allocator OOM (the
    // guest panic -> `wasi::exit(2)` seen in the build-catalog job). Raised
    // 64 -> 192 MiB (registry#473): audio.transcribe-speech accepts up to 30s
    // of 16kHz audio as a JSON float array (up to 480,000 numbers) -- for a
    // real 11s clip (176,000 samples) alone, this genuinely OOM'd at 64 MiB
    // (verified via wasmtime: exit(2), the same guest-panic path as the
    // #384 incident), not because of that capability's own ~41 MiB of
    // audio/attention scratch buffers, but because the JSON parser's
    // `Vec<Value>` for a long array is grown by repeated reallocation
    // (doesn't know the final length up front) -- and every abandoned
    // smaller buffer from each doubling stays allocated forever in a bump
    // allocator that never frees, multiplying the effective cost of a large
    // parsed array well beyond its final size. Zero-initialized static data
    // costs nothing in the compiled .wasm (no data-segment bytes for zeros)
    // and a declared-but-unwritten linear memory reservation is cheap at
    // instantiation, so this headroom is free for every capability that
    // doesn't need it.
    const HEAP_SIZE: usize = 192 * (1 << 20);

    #[repr(align(16))]
    struct AlignedHeap(UnsafeCell<[u8; HEAP_SIZE]>);
    // Safety: this WASI command is single-threaded (one instantiation, one
    // invocation, then `proc_exit`) -- there is no concurrent access.
    unsafe impl Sync for AlignedHeap {}

    static HEAP: AlignedHeap = AlignedHeap(UnsafeCell::new([0u8; HEAP_SIZE]));

    struct BumpAllocator {
        next: UnsafeCell<usize>,
    }
    unsafe impl Sync for BumpAllocator {}

    unsafe impl GlobalAlloc for BumpAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let heap_start = HEAP.0.get() as usize;
            let next_ptr = self.next.get();
            let current = if *next_ptr == 0 {
                heap_start
            } else {
                *next_ptr
            };
            let align = layout.align();
            let aligned = (current + align - 1) & !(align - 1);
            let end = match aligned.checked_add(layout.size()) {
                Some(end) => end,
                None => return core::ptr::null_mut(),
            };
            if end > heap_start + HEAP_SIZE {
                core::ptr::null_mut()
            } else {
                *next_ptr = end;
                aligned as *mut u8
            }
        }

        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
            // Bump allocator: never frees. Fine for a single-shot WASI
            // command process that exits right after producing its output.
        }
    }

    #[global_allocator]
    static ALLOCATOR: BumpAllocator = BumpAllocator {
        next: UnsafeCell::new(0),
    };
}

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // No unwinding machinery is linked in (no_std, panic = "abort"); exit
    // with a distinct non-zero code rather than looping forever, so a
    // caller can at least tell a panic occurred instead of hanging.
    wasi::exit(2)
}

#[cfg(target_arch = "wasm32")]
mod wasi {
    use alloc::vec::Vec;

    #[repr(C)]
    pub struct IoVec {
        pub buffer: *const u8,
        pub length: usize,
    }

    #[repr(C)]
    pub struct IoVecMut {
        pub buffer: *mut u8,
        pub length: usize,
    }

    #[link(wasm_import_module = "wasi_snapshot_preview1")]
    unsafe extern "C" {
        fn fd_read(fd: u32, iovs: *const IoVecMut, iovs_len: usize, nread: *mut usize) -> u32;
        fn fd_write(fd: u32, iovs: *const IoVec, iovs_len: usize, nwritten: *mut usize) -> u32;
        fn proc_exit(code: u32) -> !;
    }

    pub fn read_stdin_all() -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        loop {
            let iov = IoVecMut {
                buffer: chunk.as_mut_ptr(),
                length: chunk.len(),
            };
            let mut n: usize = 0;
            let ret = unsafe { fd_read(0, &iov, 1, &mut n) };
            if ret != 0 || n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        buf
    }

    pub fn write_stdout_all(bytes: &[u8]) {
        let mut offset = 0;
        while offset < bytes.len() {
            let iov = IoVec {
                buffer: unsafe { bytes.as_ptr().add(offset) },
                length: bytes.len() - offset,
            };
            let mut n: usize = 0;
            let ret = unsafe { fd_write(1, &iov, 1, &mut n) };
            if ret != 0 || n == 0 {
                break;
            }
            offset += n;
        }
    }

    pub fn exit(code: u32) -> ! {
        unsafe { proc_exit(code) }
    }
}

/// Read stdin, parse it as JSON, hand the parsed value to `process`, write
/// the returned value back to stdout as JSON, and exit 0. Malformed input
/// falls back to an empty object rather than panicking, so a capability's
/// own contract-level validation is what's expected to catch bad input --
/// this shim's job is plumbing, not schema enforcement.
#[cfg(target_arch = "wasm32")]
pub fn run_capability<F: FnOnce(Value) -> Value>(process: F) -> ! {
    use alloc::vec::Vec;

    let input_bytes = wasi::read_stdin_all();
    let input_str = core::str::from_utf8(&input_bytes).unwrap_or("");
    let input_value = json::parse(input_str).unwrap_or_else(|_| Value::Object(Vec::new()));
    let output_value = process(input_value);
    let output_string = json::write(&output_value);
    wasi::write_stdout_all(output_string.as_bytes());
    wasi::exit(0)
}
